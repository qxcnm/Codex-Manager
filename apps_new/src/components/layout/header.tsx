"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { Settings as SettingsIcon } from "lucide-react";
import { toast } from "sonner";
import { useAppStore } from "@/lib/store/useAppStore";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { WebPasswordModal } from "../modals/web-password-modal";
import { serviceClient } from "@/lib/api/service-client";
import { appClient } from "@/lib/api/app-client";
import { normalizeAppPath } from "@/lib/utils/navigation";
import {
  formatServiceError,
  isExpectedInitializeResult,
  normalizeServiceAddr,
} from "@/lib/utils/service";

const DEFAULT_SERVICE_ADDR = "localhost:48760";

export function Header() {
  const { serviceStatus, setServiceStatus, setAppSettings } = useAppStore();
  const pathname = normalizeAppPath(usePathname());
  const [webPasswordModalOpen, setWebPasswordModalOpen] = useState(false);
  const [isToggling, setIsToggling] = useState(false);
  const [portInput, setPortInput] = useState("48760");

  useEffect(() => {
    const current = String(serviceStatus.addr || DEFAULT_SERVICE_ADDR);
    const [, port = current] = current.split(":");
    setPortInput(port || "48760");
  }, [serviceStatus.addr]);

  const getPageTitle = () => {
    switch (pathname) {
      case "/":
        return "仪表盘";
      case "/accounts":
        return "账号管理";
      case "/apikeys":
        return "平台密钥";
      case "/logs":
        return "请求日志";
      case "/settings":
        return "应用设置";
      default:
        return "CodexManager";
    }
  };

  const persistServiceAddr = async (nextAddr: string) => {
    const normalized = normalizeServiceAddr(nextAddr);
    const settings = await appClient.setSettings({ serviceAddr: normalized });
    setAppSettings(settings);
    setServiceStatus({ addr: normalized });
    return normalized;
  };

  const handleToggleService = async (enabled: boolean) => {
    setIsToggling(true);
    try {
      const nextAddr = await persistServiceAddr(serviceStatus.addr || `localhost:${portInput}`);
      if (enabled) {
        await serviceClient.start(nextAddr);
        const initResult = await serviceClient.initialize();
        if (!isExpectedInitializeResult(initResult)) {
          throw new Error("Port is in use or unexpected service responded (missing server_name)");
        }
        setServiceStatus({
          connected: true,
          version: initResult.version,
          addr: nextAddr,
        });
        toast.success("服务已启动");
      } else {
        await serviceClient.stop();
        setServiceStatus({ connected: false, version: "" });
        toast.info("服务已停止");
      }
    } catch (error: unknown) {
      toast.error(`操作失败: ${formatServiceError(error)}`);
    } finally {
      setIsToggling(false);
    }
  };

  const handlePortBlur = async () => {
    try {
      const nextAddr = await persistServiceAddr(`localhost:${portInput}`);
      setServiceStatus({ addr: nextAddr });
    } catch (error: unknown) {
      toast.error(`地址保存失败: ${formatServiceError(error)}`);
    }
  };

  return (
    <>
      <header className="sticky top-0 z-30 flex h-16 items-center justify-between glass-header px-6">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold">{getPageTitle()}</h1>
          <Badge variant={serviceStatus.connected ? "default" : "secondary"} className="h-5">
            {serviceStatus.connected ? "服务已连接" : "服务未连接"}
          </Badge>
          {serviceStatus.version ? (
            <span className="text-xs text-muted-foreground">v{serviceStatus.version}</span>
          ) : null}
        </div>

        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2 rounded-lg border bg-card/30 px-3 py-1.5 shadow-sm">
            <span className="text-xs font-medium text-muted-foreground">监听端口</span>
            <Input
              className="h-7 w-16 border-none bg-transparent p-0 text-xs font-mono focus-visible:ring-0"
              placeholder="48760"
              value={portInput}
              onChange={(event) => {
                const nextPort = event.target.value.replace(/[^\d]/g, "");
                setPortInput(nextPort);
                if (nextPort) {
                  setServiceStatus({ addr: `localhost:${nextPort}` });
                }
              }}
              onBlur={() => void handlePortBlur()}
            />
            <div className="mx-1 h-4 w-px bg-border" />
            <Switch
              checked={serviceStatus.connected}
              disabled={isToggling}
              onCheckedChange={handleToggleService}
              className="scale-90"
            />
          </div>

          <Button
            variant="outline"
            size="sm"
            className="h-9 gap-2 px-3"
            onClick={() => setWebPasswordModalOpen(true)}
          >
            <SettingsIcon className="h-3.5 w-3.5" />
            <span className="text-xs">Web 密码</span>
          </Button>
        </div>
      </header>

      <WebPasswordModal
        open={webPasswordModalOpen}
        onOpenChange={setWebPasswordModalOpen}
      />
    </>
  );
}
