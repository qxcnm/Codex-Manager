function fallbackCopyWithExecCommand(text) {
  if (typeof document === "undefined" || !document.body) {
    return false;
  }
  const input = document.createElement("textarea");
  input.value = text;
  input.setAttribute("readonly", "readonly");
  input.style.position = "fixed";
  input.style.opacity = "0";
  input.style.pointerEvents = "none";
  document.body.appendChild(input);
  input.select();
  input.setSelectionRange(0, input.value.length);
  try {
    return document.execCommand("copy");
  } catch (_err) {
    return false;
  } finally {
    document.body.removeChild(input);
  }
}

export async function copyText(value) {
  const text = String(value || "");
  if (!text) {
    return false;
  }

  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_err) {
      // 中文注释：浏览器权限策略在某些上下文会拒绝 clipboard API；这里回退到 execCommand 兼容旧环境。
    }
  }

  return fallbackCopyWithExecCommand(text);
}
