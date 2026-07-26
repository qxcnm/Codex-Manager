"use client";

import type { MessageCatalog } from "../types";

export const KO_PROJECTS_MESSAGES: MessageCatalog = {
  项目启动: "프로젝트 실행",
  "收藏常用目录，在 Windows 与 macOS 上直接用 ChatGPT Codex App 打开项目。":
    "자주 쓰는 폴더를 저장하고 Windows 및 macOS에서 ChatGPT Codex App으로 프로젝트를 바로 엽니다.",
  "{count} 个项目": "프로젝트 {count}개",
  添加目录: "폴더 추가",
  目录可用: "사용 가능",
  目录不可用: "폴더를 사용할 수 없음",
  "目录可能已移动或删除；你仍可以安全移除这条记录。":
    "폴더가 이동되었거나 삭제되었을 수 있습니다. 이 항목은 안전하게 제거할 수 있습니다.",
  启动: "실행",
  会话: "세션",
  移除: "제거",
  "本机 Codex CLI": "로컬 Codex CLI",
  "Codex App 与本机 CLI": "Codex App 및 로컬 CLI",
  "Windows 与 macOS 的“启动”会在 ChatGPT Codex App 中打开项目；“会话”继续使用本机 Codex CLI 会话选择器。":
    "Windows 및 macOS에서 실행은 ChatGPT Codex App에서 프로젝트를 열고, 세션은 로컬 Codex CLI 세션 선택기를 계속 사용합니다.",
  "启动时会把项目设为工作目录，并优先使用本机 CodexManager 保存的 Codex profile；未配置时沿用本机 CODEX_HOME。":
    "실행할 때 프로젝트를 작업 디렉터리로 설정하고 로컬 CodexManager에 저장된 Codex 프로필을 우선 사용합니다. 설정하지 않은 경우 로컬 CODEX_HOME을 사용합니다.",
  "远程服务上的 Codex profile 不会复制到本机，也不会作为本机启动路径使用。":
    "원격 서비스의 Codex 프로필은 이 기기로 복사되지 않으며 로컬 실행 경로로 사용되지 않습니다.",
  "“会话”会打开 Codex CLI 自带的当前项目会话选择器，不会由 CodexManager 读取或修改会话文件。":
    "세션은 현재 프로젝트용 Codex CLI 세션 선택기를 엽니다. CodexManager는 세션 파일을 읽거나 수정하지 않습니다.",
  项目启动仅支持桌面端: "프로젝트 실행은 데스크톱에서만 지원됩니다",
  "Web / Docker 无法安全打开你设备上的目录和交互式终端。":
    "Web 및 Docker 빌드는 기기의 폴더와 대화형 터미널을 안전하게 열 수 없습니다.",
  项目列表加载失败: "프로젝트 목록을 불러오지 못했습니다",
  还没有项目目录: "아직 프로젝트 폴더가 없습니다",
  "添加一个本机目录，即可从这里启动 Codex 或继续该项目的会话。":
    "로컬 폴더를 추가하면 여기에서 Codex를 실행하거나 해당 프로젝트의 세션을 계속할 수 있습니다.",
  项目目录已添加: "프로젝트 폴더를 추가했습니다",
  该目录已在项目列表中: "이 폴더는 이미 프로젝트 목록에 있습니다",
  添加目录失败: "폴더 추가 실패",
  项目记录已移除: "프로젝트 항목을 제거했습니다",
  项目记录已不存在: "프로젝트 항목이 더 이상 존재하지 않습니다",
  移除失败: "프로젝트 제거 실패",
  "已请求打开 Codex 会话选择器": "Codex 세션 선택기 열기를 요청했습니다",
  "已请求在新终端中启动 Codex": "새 터미널에서 Codex 실행을 요청했습니다",
  "已在 ChatGPT Codex App 中打开项目": "ChatGPT Codex App에서 프로젝트를 열었습니다",
  "启动 Codex 失败": "Codex 실행 실패",
  移除项目记录: "프로젝트 항목 제거",
  "只会从 CodexManager 中移除“{name}”，不会删除项目目录或其中的文件。":
    "CodexManager에서 “{name}” 항목만 제거합니다. 프로젝트 폴더와 파일은 삭제되지 않습니다.",
  确认移除: "항목 제거",
};
