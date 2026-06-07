"use client";

import type { MessageCatalog } from "../types";

export const KO_PLATFORM_MODE_MESSAGES: MessageCatalog = {
  平台模式选择: "플랫폼 모드 선택",
  "选择 Codex CLI 直连账号，或通过 CodexManager 本地网关接入。":
    "Codex CLI 계정 직결 또는 CodexManager 로컬 게이트웨이 경유 방식을 선택합니다.",
  写入位置说明: "쓰기 위치 안내",
  "这里修改的是 codexmanager-service 所在机器的 Codex 配置目录，不一定是当前浏览器所在机器。":
    "여기서 바꾸는 것은 codexmanager-service 가 실행 중인 머신의 Codex 설정 디렉터리이며, 현재 브라우저가 실행 중인 머신과 다를 수 있습니다.",
  "Web / Docker 模式": "Web / Docker 모드",
  "当前页面会通过 /api/rpc 写入 codexmanager-service 进程可访问的 Codex profile；Docker 部署时请确认 CODEX_HOME 或挂载卷指向你希望 Codex CLI 使用的配置目录。":
    "이 페이지는 /api/rpc 를 통해 codexmanager-service 프로세스가 접근할 수 있는 Codex profile에 씁니다. Docker 배포에서는 CODEX_HOME 또는 마운트된 볼륨이 Codex CLI가 사용할 설정 디렉터리를 가리키는지 확인하세요.",
  "当前运行环境无法访问管理 RPC，暂时不能读取或写入 Codex profile。":
    "현재 실행 환경에서는 관리 RPC에 접근할 수 없어 지금은 Codex profile을 읽거나 쓸 수 없습니다.",
  "Profile 迁移警告": "Profile 마이그레이션 경고",
  "当前模式": "현재 모드",
  "Codex profile": "Codex profile",
  当前账号: "현재 계정",
  "当前平台 Key": "현재 플랫폼 키",
  "最后应用": "마지막 적용",
  刷新状态: "상태 새로고침",
  "正在使用": "사용 중",
  账号直连: "계정 직결",
  "OpenAI 账号": "OpenAI 계정",
  选择账号: "계정 선택",
  "直连 OpenAI 官方后端，不经过 CodexManager 网关；不会产生 CodexManager 请求日志，仪表盘用量统计不可用。":
    "CodexManager 게이트웨이를 거치지 않고 OpenAI 공식 백엔드에 직접 연결합니다. CodexManager 요청 로그와 대시보드 사용량 통계는 사용할 수 없습니다.",
  "没有可用于账号直连的 active OpenAI 账号。":
    "계정 직결에 사용할 수 있는 활성 OpenAI 계정이 없습니다.",
  "去添加 OpenAI 账号": "OpenAI 계정 추가",
  "正在读取可用账号...": "사용 가능한 계정을 불러오는 중...",
  "可用账号数：{count}": "사용 가능한 계정 수: {count}",
  "重新应用账号直连": "계정 직결 다시 적용",
  "切换到账号直连": "계정 직결로 전환",
  本地网关: "로컬 게이트웨이",
  "通过 CodexManager 本地网关转发 Codex CLI 请求；请求日志、Token、费用估算和仪表盘统计可用。":
    "Codex CLI 요청을 CodexManager 로컬 게이트웨이로 전달합니다. 요청 로그, 토큰, 비용 추정, 대시보드 통계를 사용할 수 있습니다.",
  "没有可用于本地网关的平台密钥。":
    "로컬 게이트웨이에 사용할 수 있는 플랫폼 키가 없습니다.",
  "去创建平台密钥": "플랫폼 키 생성",
  "选择平台密钥": "플랫폼 키 선택",
  "将使用 gateway base_url": "사용할 gateway base_url",
  "重新应用本地网关": "로컬 게이트웨이 다시 적용",
  "切换到本地网关": "로컬 게이트웨이로 전환",
  "保存失败": "저장 실패",
  "切换失败": "전환 실패",
  "修复失败": "복구 실패",
  "恢复失败": "복원 실패",
  "清理完成但有警告": "정리가 완료되었지만 경고가 있습니다",
  "历史修复完成但有警告": "기록 복구가 완료되었지만 경고가 있습니다",
  "历史会话可见性已修复": "기록 세션 가시성이 복구되었습니다",
  "历史会话已与当前模式一致": "기록 세션이 이미 현재 모드와 일치합니다",
  "Codex profile 路径已保存": "Codex profile 경로가 저장되었습니다",
  "已切换到账号直连": "계정 직결로 전환되었습니다",
  "已切换到本地网关": "로컬 게이트웨이로 전환되었습니다",
  "已恢复接管前的 Codex 配置": "관리 전 Codex 설정이 복원되었습니다",
  "已清理 {count} 份历史备份，释放 {bytes}":
    "기록 백업 {count}개를 정리하고 {bytes}를 해제했습니다",
  "高级与恢复": "고급 및 복구",
  "修改 profile 目录、gateway base_url、修复历史会话或恢复接管前配置。":
    "profile 디렉터리, gateway base_url, 기록 세션 복구, 기존 관리 전 설정 복원을 조정합니다.",
  "Profile 目标目录": "Profile 대상 디렉터리",
  "默认使用 CODEX_HOME 或 service 用户的 ~/.codex。":
    "기본적으로 CODEX_HOME 또는 service 사용자의 ~/.codex 를 사용합니다.",
  "Codex profile 目录": "Codex profile 디렉터리",
  "OpenAI gateway base_url": "OpenAI gateway base_url",
  "Gateway base_url": "Gateway base_url",
  "auth.json": "auth.json",
  "config.toml": "config.toml",
  "CodexManager 管理文件": "CodexManager 관리 파일",
  管理标记: "관리 마커",
  可写: "쓰기 가능",
  是: "예",
  "否或未知": "아니오 또는 알 수 없음",
  "默认使用当前 Web 服务可访问的本地网关地址。":
    "기본적으로 현재 Web 서비스에서 접근 가능한 로컬 게이트웨이 주소를 사용합니다.",
  "使用当前网关": "현재 게이트웨이 사용",
  "恢复与历史会话": "복구 및 기록 세션",
  "切换模式时会自动修复历史会话 provider 元数据；Codex 运行中锁库时可手动重试。":
    "모드를 전환하면 기록 세션의 provider 메타데이터를 자동으로 복구합니다. Codex가 DB를 잠그고 있으면 종료 후 다시 시도하세요.",
  "历史会话可见性": "기록 세션 가시성",
  "切换 direct / gateway 时会自动修复历史会话的 provider 元数据。":
    "direct / gateway 전환 시 기록 세션의 provider 메타데이터를 자동으로 복구합니다.",
  "修复历史可见性": "기록 가시성 복구",
  "目标 provider": "대상 provider",
  "已修复 rollout / SQLite / session_index": "복구된 rollout / SQLite / session_index",
  备份目录: "백업 디렉터리",
  警告: "경고",
  "历史修复备份": "기록 복구 백업",
  "备份保存在 CodexManager 数据目录，不再写入 Codex profile。":
    "백업은 CodexManager 데이터 디렉터리에 저장되며 Codex profile에는 더 이상 쓰지 않습니다.",
  "清理历史备份": "기록 백업 정리",
  "数量 / 占用": "개수 / 사용량",
  保留策略: "보관 정책",
  备份: "백업",
  已保存: "저장됨",
  暂无: "없음",
  "最多 {count} 份，最多 {days} 天，至少保留最新 {min} 份":
    "최대 {count}개, 최대 {days}일 보관하며, 최신 {min}개는 최소 보존합니다.",
  "恢复接管前配置": "관리 전 설정 복원",
};
