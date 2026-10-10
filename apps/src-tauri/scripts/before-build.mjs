import { existsSync, rmSync } from "node:fs";
import http from "node:http";
import { dirname, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import net from "node:net";
import { PassThrough } from "node:stream";
import {
  findDesktopDevProcess,
  getDesktopDevProcessInfo,
  listDesktopDevListenerPids,
  terminateDesktopDevProcessTree,
} from "./desktop-dev-processes.mjs";

const cwd = process.cwd();
const task = process.argv[2] || "build:desktop";
const desktopDevHost = "127.0.0.1";
const desktopDevPort = 3005;
const desktopNextPort = 3006;
const desktopDevWaitTimeoutMs = 10_000;
const desktopDevWaitIntervalMs = 500;
const desktopDevWarmupTimeoutMs = 120_000;
const candidates = [
  cwd,
  resolve(cwd, "apps"),
  resolve(cwd, "..", "apps"),
  resolve(cwd, "..", "..", "apps"),
  resolve(cwd, ".."),
  resolve(cwd, "..", ".."),
];

function hasFrontendPackage(dir) {
  return existsSync(resolve(dir, "package.json"));
}

function hasBuiltFrontendDist(dir) {
  return existsSync(resolve(dir, "out", "index.html"));
}

function canConnect(host, port, timeoutMs = 1000) {
  return new Promise((resolvePromise) => {
    const socket = new net.Socket();
    let settled = false;

    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      socket.destroy();
      resolvePromise(result);
    };

    socket.setTimeout(timeoutMs);
    socket.once("connect", () => finish(true));
    socket.once("timeout", () => finish(false));
    socket.once("error", () => finish(false));
    socket.connect(port, host);
  });
}

async function hasReusableDesktopDevServer() {
  const reachable = await canConnect(desktopDevHost, desktopDevPort);
  if (!reachable) {
    return false;
  }

  try {
    const response = await fetch(`http://${desktopDevHost}:${desktopDevPort}/`, {
      signal: AbortSignal.timeout(1500),
    });
    return response.ok;
  } catch {
    return false;
  }
}

function getDesktopDevLockPath(dir) {
  return resolve(dir, ".next", "dev", "lock");
}

function sleep(ms) {
  return new Promise((resolvePromise) => {
    setTimeout(resolvePromise, ms);
  });
}

async function waitForDesktopDevPortsToClose(timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() <= deadline) {
    const proxyClosed = !(await canConnect(desktopDevHost, desktopDevPort, 300));
    const nextClosed = !(await canConnect(desktopDevHost, desktopNextPort, 300));
    if (proxyClosed && nextClosed) {
      return true;
    }

    if (Date.now() >= deadline) {
      return false;
    }

    await sleep(250);
  }

  return false;
}

async function waitForReusableDesktopDevServer(timeoutMs = desktopDevWaitTimeoutMs) {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() <= deadline) {
    if (await hasReusableDesktopDevServer()) {
      return true;
    }

    if (Date.now() >= deadline) {
      return false;
    }

    await sleep(desktopDevWaitIntervalMs);
  }

  return false;
}

async function fetchDesktopDevPath(path, timeoutMs, port = desktopDevPort) {
  const response = await fetch(`http://${desktopDevHost}:${port}${path}`, {
    signal: AbortSignal.timeout(timeoutMs),
  });
  await response.arrayBuffer();
  return response.ok;
}

async function waitForDesktopAppPage() {
  const deadline = Date.now() + desktopDevWarmupTimeoutMs;
  while (Date.now() <= deadline) {
    try {
      if (await fetchDesktopDevPath("/", 1500, desktopNextPort)) {
        console.log(`前端首页已就绪: http://${desktopDevHost}:${desktopNextPort}/`);
        return;
      }
    } catch {
      // Keep waiting until Next starts serving static files.
    }

    if (Date.now() >= deadline) {
      break;
    }

    await sleep(desktopDevWaitIntervalMs);
  }

  throw new Error(`等待前端首页就绪超时: http://${desktopDevHost}:${desktopNextPort}/`);
}

function isExpectedDesktopDevProxyDisconnect(error) {
  return ["ECONNABORTED", "ECONNRESET", "EPIPE", "ERR_STREAM_DESTROYED"].includes(error?.code);
}

function respondWithProxyNotReady(response) {
  if (response.destroyed || response.headersSent) {
    return;
  }
  response.writeHead(503, {
    "content-type": "text/plain; charset=utf-8",
    "retry-after": "1",
  });
  response.end("Frontend dev server is not ready yet");
}

function createDesktopDevProxy(nextReadyPromise) {
  const server = http.createServer((request, response) => {
    const gatedRequest = new PassThrough();
    request.pipe(gatedRequest);

    void nextReadyPromise.then(
      () => {
        if (request.aborted || response.destroyed) {
          gatedRequest.destroy();
          return;
        }

        const proxyRequest = http.request(
          {
            hostname: desktopDevHost,
            port: desktopNextPort,
            path: request.url,
            method: request.method,
            headers: {
              ...request.headers,
              host: `${desktopDevHost}:${desktopNextPort}`,
            },
          },
          (proxyResponse) => {
            if (response.destroyed) {
              proxyResponse.destroy();
              return;
            }
            response.writeHead(proxyResponse.statusCode ?? 502, proxyResponse.headers);
            proxyResponse.on("error", (error) => {
              response.destroy();
              if (!isExpectedDesktopDevProxyDisconnect(error)) {
                console.warn(`Next dev proxy response failed: ${error.message}`);
              }
            });
            proxyResponse.pipe(response);
          },
        );

        proxyRequest.on("error", (error) => {
          if (response.destroyed || isExpectedDesktopDevProxyDisconnect(error)) {
            response.destroy();
            return;
          }
          if (response.headersSent) {
            console.warn(`Next dev proxy failed after sending headers: ${error.message}`);
            response.destroy();
            return;
          }
          response.writeHead(502, { "content-type": "text/plain; charset=utf-8" });
          response.end(`Next dev proxy error: ${error.message}`);
        });

        request.on("aborted", () => {
          proxyRequest.destroy();
        });
        request.on("error", (error) => {
          proxyRequest.destroy();
          if (!isExpectedDesktopDevProxyDisconnect(error)) {
            console.warn(`Desktop dev proxy request failed: ${error.message}`);
          }
        });
        response.on("error", (error) => {
          proxyRequest.destroy();
          if (!isExpectedDesktopDevProxyDisconnect(error)) {
            console.warn(`Desktop dev proxy client response failed: ${error.message}`);
          }
        });

        gatedRequest.pipe(proxyRequest);
      },
      () => {
        gatedRequest.destroy();
        request.resume();
        respondWithProxyNotReady(response);
      },
    );

    request.on("aborted", () => {
      gatedRequest.destroy();
    });
    request.on("error", (error) => {
      gatedRequest.destroy();
      if (!isExpectedDesktopDevProxyDisconnect(error)) {
        console.warn(`Desktop dev proxy request failed: ${error.message}`);
      }
    });
  });

  // Reloading a WebView closes its HTTP/HMR sockets immediately. On Windows
  // this commonly surfaces as ECONNABORTED on the raw client socket; consume
  // that expected disconnect so it does not terminate beforeDevCommand.
  server.on("connection", (socket) => {
    socket.on("error", (error) => {
      if (!isExpectedDesktopDevProxyDisconnect(error)) {
        console.warn(`Desktop dev proxy client socket failed: ${error.message}`);
      }
    });
  });

  server.on("upgrade", (request, socket, head) => {
    void nextReadyPromise.then(
      () => {
        if (socket.destroyed) {
          return;
        }
        const upstream = net.connect(desktopNextPort, desktopDevHost, () => {
          upstream.write(
            [
              `${request.method} ${request.url} HTTP/${request.httpVersion}`,
              `Host: ${desktopDevHost}:${desktopNextPort}`,
              ...Object.entries(request.headers)
                .filter(([key]) => key.toLowerCase() !== "host")
                .map(([key, value]) => `${key}: ${value}`),
              "",
              "",
            ].join("\r\n"),
          );
          if (head.length > 0) {
            upstream.write(head);
          }
          upstream.pipe(socket);
          socket.pipe(upstream);
        });

        upstream.on("error", () => {
          socket.destroy();
        });
        socket.on("close", () => {
          upstream.destroy();
        });
      },
      () => {
        socket.destroy();
      },
    );
  });

  server.on("error", (error) => {
    console.error(`前端开发代理启动失败: ${error.message}`);
    process.exit(1);
  });

  server.listen(desktopDevPort, desktopDevHost, () => {
    console.log(
      `前端开发代理已就绪: http://${desktopDevHost}:${desktopDevPort} -> http://${desktopDevHost}:${desktopNextPort}`,
    );
  });

  return server;
}

async function cleanupStaleDesktopDevState() {
  const occupiedPorts = [];
  if (await canConnect(desktopDevHost, desktopDevPort, 300)) {
    occupiedPorts.push(desktopDevPort);
  }
  if (await canConnect(desktopDevHost, desktopNextPort, 300)) {
    occupiedPorts.push(desktopNextPort);
  }
  const listeners = occupiedPorts.flatMap((port) =>
    listDesktopDevListenerPids(port, desktopDevHost).map((pid) => ({ pid, port })),
  );

  const unresolvedOccupiedPorts = occupiedPorts.filter(
    (port) => !listeners.some((listener) => listener.port === port),
  );
  const stillUnresolvedOccupiedPorts = [];
  for (const port of unresolvedOccupiedPorts) {
    if (await canConnect(desktopDevHost, port, 300)) {
      stillUnresolvedOccupiedPorts.push(port);
    }
  }
  if (stillUnresolvedOccupiedPorts.length > 0) {
    console.error(
      `检测到开发端口被占用，但无法识别监听进程。请手动释放端口: ${stillUnresolvedOccupiedPorts.join(", ")}`,
    );
    process.exit(1);
  }

  const desktopDevProcesses = new Map();

  for (const listener of listeners) {
    const listenerProcessInfo = getDesktopDevProcessInfo(listener.pid);
    const desktopDevProcess =
      findDesktopDevProcess(listener.pid, {
        port: listener.port,
        frontendDir,
      }) ||
      findDesktopDevProcess(listener.pid, {
        port: listener.port === desktopDevPort ? desktopNextPort : desktopDevPort,
        frontendDir,
      });
    if (!desktopDevProcess) {
      console.error(
        `开发端口被其他进程占用，无法自动清理。PID: ${listener.pid}，命令行: ${listenerProcessInfo?.CommandLine || "未知"}`,
      );
      process.exit(1);
    }

    desktopDevProcesses.set(desktopDevProcess.pid, desktopDevProcess);
  }

  for (const { pid } of desktopDevProcesses.values()) {
    console.log(`检测到未响应的 Next.js 开发进程，准备终止: PID ${pid}`);
    if (!terminateDesktopDevProcessTree(pid)) {
      console.error(`终止残留 Next.js 开发进程失败: PID ${pid}`);
      process.exit(1);
    }
  }

  if (listeners.length > 0) {
    const portReleased = await waitForDesktopDevPortsToClose();
    if (!portReleased) {
      console.error(`开发端口释放超时，无法继续启动前端开发服务`);
      process.exit(1);
    }
  }

  const desktopDevLockPath = getDesktopDevLockPath(frontendDir);
  if (existsSync(desktopDevLockPath)) {
    try {
      rmSync(desktopDevLockPath, { force: true });
      console.log(`已清理未响应的 Next.js 开发锁文件: ${desktopDevLockPath}`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error(`清理 Next.js 开发锁文件失败: ${message}`);
      process.exit(1);
    }
  }
}

function resolvePnpmCommand() {
  const baseArgs =
    task === "dev:desktop"
      ? [
          "--dir",
          frontendDir,
          "exec",
          "next",
          "dev",
          "--webpack",
          "-H",
          desktopDevHost,
          "-p",
          String(desktopNextPort),
        ]
      : ["--dir", frontendDir, "run", task];
  const nodeBinDir = dirname(process.execPath);
  const windowsShell = process.env.ComSpec || "cmd.exe";
  // cmd.exe consumes the /c remainder as one command string, so quote every token explicitly.
  const quoteWindowsCommandArgument = (value) => `"${String(value).replaceAll('"', '""')}"`;
  const buildWindowsCommandLine = (command, args) =>
    `"${[command, ...args].map(quoteWindowsCommandArgument).join(" ")}"`;
  const wrapWindowsCommand = (
    command,
    commandArgs = baseArgs,
    probeCommandArgs = ["--version"],
  ) => ({
    command: windowsShell,
    targetCommand: command,
    args: ["/d", "/s", "/c", buildWindowsCommandLine(command, commandArgs)],
    probeArgs: ["/d", "/s", "/c", buildWindowsCommandLine(command, probeCommandArgs)],
    shell: false,
    windowsVerbatimArguments: true,
  });
  const windowsCandidates = [
    wrapWindowsCommand(resolve(nodeBinDir, "pnpm.cmd")),
    wrapWindowsCommand(
      resolve(nodeBinDir, "corepack.cmd"),
      ["pnpm", ...baseArgs],
      ["pnpm", "--version"],
    ),
    wrapWindowsCommand("pnpm.cmd"),
    wrapWindowsCommand("corepack.cmd", ["pnpm", ...baseArgs], ["pnpm", "--version"]),
  ];
  const defaultCandidates = [
    { command: "pnpm", args: baseArgs, probeArgs: ["--version"], shell: false },
    { command: "corepack", args: ["pnpm", ...baseArgs], probeArgs: ["pnpm", "--version"], shell: false },
  ];

  const candidates = process.platform === "win32" ? windowsCandidates : defaultCandidates;
  const existingPathCandidates = candidates.filter(
    (candidate) =>
      !candidate.targetCommand ||
      !candidate.targetCommand.includes(":") ||
      existsSync(candidate.targetCommand),
  );

  for (const candidate of existingPathCandidates) {
    const probeArgs = candidate.probeArgs || ["--version"];
    const probe = spawnSync(candidate.command, probeArgs, {
      encoding: "utf8",
      shell: candidate.shell ?? false,
      windowsVerbatimArguments: candidate.windowsVerbatimArguments ?? false,
      stdio: "ignore",
    });
    if (!probe.error && probe.status === 0) {
      return candidate;
    }
  }

  return candidates[candidates.length - 1];
}

const frontendDir = candidates.find(hasFrontendPackage);
if (!frontendDir) {
  console.error(`前端项目目录不存在，当前工作目录: ${cwd}`);
  process.exit(1);
}

if (task === "build:desktop" && hasBuiltFrontendDist(frontendDir)) {
  console.log(`前端产物已存在，跳过重复构建: ${resolve(frontendDir, "out", "index.html")}`);
  process.exit(0);
}

if (task === "dev:desktop") {
  if (await hasReusableDesktopDevServer()) {
    console.log(`检测到现有前端开发服务，直接复用: http://${desktopDevHost}:${desktopDevPort}`);
    process.exit(0);
  }

  const desktopDevLockPath = getDesktopDevLockPath(frontendDir);
  const hasDesktopDevLock = existsSync(desktopDevLockPath);
  const hasDesktopDevPortListener = await canConnect(desktopDevHost, desktopDevPort, 300);
  const hasDesktopNextPortListener = await canConnect(desktopDevHost, desktopNextPort, 300);
  if (hasDesktopDevLock || hasDesktopDevPortListener || hasDesktopNextPortListener) {
    const staleState = [
      hasDesktopDevLock ? "锁文件" : null,
      hasDesktopDevPortListener ? `${desktopDevPort}端口占用` : null,
      hasDesktopNextPortListener ? `${desktopNextPort}端口占用` : null,
    ]
      .filter(Boolean)
      .join(" / ");
    console.log(`检测到 Next.js 开发态残留（${staleState}），等待现有实例就绪: ${desktopDevLockPath}`);

    if (await waitForReusableDesktopDevServer()) {
      console.log(`检测到现有前端开发服务，直接复用: http://${desktopDevHost}:${desktopDevPort}`);
      process.exit(0);
    }

    await cleanupStaleDesktopDevState();
  }
}

const packageManager = resolvePnpmCommand();
console.log(`执行前端任务: ${packageManager.command} ${packageManager.args.join(" ")}`);
const needsShell = process.platform === "win32" && /\.cmd$/i.test(packageManager.command);

if (task === "dev:desktop") {
  const child = spawn(packageManager.command, packageManager.args, {
    stdio: "inherit",
    shell: packageManager.shell ?? needsShell,
    windowsVerbatimArguments: packageManager.windowsVerbatimArguments ?? false,
    windowsHide: true,
  });
  child.once("error", (error) => {
    console.error(`前端开发服务启动失败: ${error.message}`);
    process.exit(1);
  });
  // Observe early exits too: startup can fail before the first page is ready.
  child.once("exit", (code, signal) => {
    process.exit(signal ? 0 : (code ?? 0));
  });
  const nextReadyPromise = waitForDesktopAppPage();
  // Bind the Tauri dev URL immediately, but hold requests until Next has
  // served the home page so the WebView cannot receive a startup 502.
  const proxy = createDesktopDevProxy(nextReadyPromise);
  try {
    await nextReadyPromise;
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    proxy.close();
    if (child.pid) terminateDesktopDevProcessTree(child.pid);
    process.exit(1);
  }
  await new Promise(() => {});
}

const result = spawnSync(packageManager.command, packageManager.args, {
  stdio: "inherit",
  shell: packageManager.shell ?? needsShell,
  windowsVerbatimArguments: packageManager.windowsVerbatimArguments ?? false,
});

if (result.error) {
  console.error(`前端构建启动失败: ${result.error.message}`);
  process.exit(1);
}

if (typeof result.status === "number" && result.status !== 0) {
  process.exit(result.status);
}

process.exit(0);
