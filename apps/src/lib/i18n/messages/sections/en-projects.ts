"use client";

import type { MessageCatalog } from "../types";

export const EN_PROJECTS_MESSAGES: MessageCatalog = {
  项目启动: "Project Launcher",
  "收藏常用目录，在 Windows 与 macOS 上直接用 ChatGPT Codex App 打开项目。":
    "Save frequently used folders and open projects directly in the ChatGPT Codex App on Windows and macOS.",
  "{count} 个项目": "{count} projects",
  添加目录: "Add folder",
  目录可用: "Available",
  目录不可用: "Folder unavailable",
  "目录可能已移动或删除；你仍可以安全移除这条记录。":
    "The folder may have been moved or deleted. You can still safely remove this entry.",
  启动: "Launch",
  会话: "Sessions",
  移除: "Remove",
  "本机 Codex CLI": "Local Codex CLI",
  "Codex App 与本机 CLI": "Codex App and local CLI",
  "Windows 与 macOS 的“启动”会在 ChatGPT Codex App 中打开项目；“会话”继续使用本机 Codex CLI 会话选择器。":
    "On Windows and macOS, Launch opens the project in the ChatGPT Codex App; Sessions continues to use the local Codex CLI session picker.",
  "启动时会把项目设为工作目录，并优先使用本机 CodexManager 保存的 Codex profile；未配置时沿用本机 CODEX_HOME。":
    "The project becomes the working directory at launch. The Codex profile stored by the local CodexManager is preferred; if none is configured, the local CODEX_HOME is used.",
  "远程服务上的 Codex profile 不会复制到本机，也不会作为本机启动路径使用。":
    "A Codex profile from a remote service is not copied to this device or used as a local launch path.",
  "“会话”会打开 Codex CLI 自带的当前项目会话选择器，不会由 CodexManager 读取或修改会话文件。":
    "Sessions opens the Codex CLI picker for the current project. CodexManager does not read or modify session files.",
  项目启动仅支持桌面端: "Project launching is desktop-only",
  "Web / Docker 无法安全打开你设备上的目录和交互式终端。":
    "Web and Docker builds cannot safely open folders and interactive terminals on your device.",
  项目列表加载失败: "Failed to load projects",
  还没有项目目录: "No project folders yet",
  "添加一个本机目录，即可从这里启动 Codex 或继续该项目的会话。":
    "Add a local folder to launch Codex or continue that project's sessions here.",
  项目目录已添加: "Project folder added",
  该目录已在项目列表中: "This folder is already in the project list",
  添加目录失败: "Failed to add folder",
  项目记录已移除: "Project entry removed",
  项目记录已不存在: "The project entry no longer exists",
  移除失败: "Failed to remove project",
  "已请求打开 Codex 会话选择器": "Requested the Codex session picker",
  "已请求在新终端中启动 Codex": "Requested Codex in a new terminal",
  "已在 ChatGPT Codex App 中打开项目": "Opened the project in the ChatGPT Codex App",
  "启动 Codex 失败": "Failed to launch Codex",
  移除项目记录: "Remove project entry",
  "只会从 CodexManager 中移除“{name}”，不会删除项目目录或其中的文件。":
    "This only removes “{name}” from CodexManager. The project folder and its files will not be deleted.",
  确认移除: "Remove entry",
};
