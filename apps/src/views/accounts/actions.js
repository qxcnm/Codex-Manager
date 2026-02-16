import { dom } from "../../ui/dom.js";

// 打开账号登录弹窗
export function openAccountModal() {
  dom.modalAccount.classList.add("active");
  dom.loginUrl.value = "";
  if (dom.manualCallbackUrl) {
    dom.manualCallbackUrl.value = "";
  }
  dom.loginHint.textContent = "点击登录后会打开浏览器完成授权。";
  dom.inputNote.value = "";
  dom.inputTags.value = "";
  dom.inputGroup.value = "TEAM";
}

// 关闭账号登录弹窗
export function closeAccountModal() {
  dom.modalAccount.classList.remove("active");
}
