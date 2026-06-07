# 문서 디렉터리 안내

`docs/`는 CodexManager의 공식 장문 문서 디렉터리입니다.

목표:
- 운영 가이드, 릴리스 문서, 유지보수 규칙을 저장소 안에서 일관되게 관리합니다.
- 새 기여자가 구두 설명 없이도 필요한 문서를 빠르게 찾을 수 있게 합니다.

## 프로젝트 개요

CodexManager는 Codex 워크플로를 위한 로컬 desktop + service-process 계정 풀 관리자이자 게이트웨이 릴레이 도구입니다.

- 계정, 사용량, 플랫폼 Key를 한 곳에서 관리합니다.
- Codex CLI, Gemini CLI, Claude Code, 서드파티 도구를 위한 로컬 OpenAI 호환 게이트웨이를 제공합니다.
- 계정 라우팅, 모델/프로필 오버라이드, aggregate API 업스트림 릴레이를 지원합니다.

## 기능 요약

- 계정 풀 관리: 그룹, 태그, 정렬, 메모, 차단 인식/필터링.
- 일괄 가져오기/내보내기: 다중 파일 가져오기, 데스크톱 폴더 재귀 가져오기, 계정 단위 내보내기.
- 사용량 표시: 5시간 + 7일 윈도우, 단일 윈도우 계정, Code Review / Spark 등 공식 추가 버킷.
- 플랫폼 Key: 생성, 비활성화, 삭제, 모델 바인딩, 추론 등급, 서비스 등급.
- Aggregate API: 서드파티 릴레이 업스트림 생성/수정/연결 테스트, 공급자명, 우선순위.
- 플러그인 센터: 내장/사설/커스텀 소스 모드, 작업/로그 화면, Rhai 연동.
- 로컬 서비스 + 게이트웨이: 바인드/리스닝 설정, 업스트림 프록시, 요청 전체 타임아웃, 스트리밍 idle 타임아웃, SSE keepalive, 통합 호환 엔드포인트.
- 이미지 생성: 기본적으로 `/v1/responses`에 공식 Codex `image_generation` tool을 자동 주입하고, 명시적으로 전달된 tool은 그대로 전달하며, `/v1/images/generations` 및 `/v1/images/edits` 호환 엔드포인트를 지원합니다. 기본 이미지 tool 모델은 `gpt-image-2`입니다.

## 빠른 시작

1. 데스크톱 앱을 실행하고 **Start Service**를 클릭합니다.
2. **Account Management**에서 계정을 추가하고 인증을 완료합니다.
3. 콜백 파싱에 실패하면 콜백 URL을 붙여넣어 수동 파싱합니다.
4. 사용량을 새로고침하고 계정 상태를 확인합니다.

## 스크린샷

![Dashboard](../../assets/images/dashboard.png)
![Account Management](../../assets/images/accounts.png)
![Platform Key](../../assets/images/platform-key.png)
![Aggregate API](../../assets/images/aggregate-api.png)
![Plugin Center](../../assets/images/plug.png)
![Log View](../../assets/images/log.png)
![Settings](../../assets/images/themes.png)

## 문서 역할
- 루트 `README.md` 및 다국어 `docs/*/README.md`: 프로젝트 개요와 빠른 시작.
- 루트 `변경-이력.md`: 버전 기록과 미출시 변경 사항.
- `report/*`: 운영, 문제 해결, 호환성 메모, FAQ.
- `release/*`: 빌드, 패키징, 배포, 산출물 문서.

## 시작 지점
- 최신 릴리스 내용과 미출시 변경 사항은 [변경-이력.md](변경-이력.md)에서 확인하세요.
- 어떤 문서를 먼저 봐야 할지 모르겠다면 아래 표를 이용하세요.

## 스폰서

CodexManager를 후원해 주신 다음 스폰서께 감사드립니다.

<table>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://apikey.fun/register?aff=Codex-Manager">
        <img src="../../assets/images/sponsors/APIKey.Fun.png" alt="APIKEY.FUN" width="120" />
      </a>
    </td>
    <td valign="top">
      <strong>APIKEY.FUN</strong> 의 프로젝트 후원에 감사드립니다! APIKEY.FUN 은 기업과 개인 개발자에게 안정적이고 효율적이며 저렴한 AI 모델 API 접속 서비스를 제공하는 전문 엔터프라이즈급 AI 중계 플랫폼입니다. Claude, OpenAI, Gemini 등 주요 인기 모델을 지원하며, 가격은 공식 가격의 7% 수준까지 낮습니다. 이 프로젝트의 <a href="https://apikey.fun/register?aff=Codex-Manager">전용 링크</a>로 가입하면 최대 <strong>충전 영구 5% 할인</strong> 전용 혜택도 받을 수 있습니다.
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://gzxsy.vip/register?aff=eapz">
        <img src="../../assets/images/sponsors/xingsiyan.jpg" alt="Xing Si Yan Gateway" width="120" />
      </a>
    </td>
    <td valign="top">
      <strong>Xing Si Yan Gateway</strong> 는 Claude Code, Codex 등 모델 호출 시나리오를 위한 안정적인 중계와 부가 서비스를 제공합니다. 고가용성 API, 편리한 도입, 지속적인 전달 지원이 필요한 개발자와 팀에 적합합니다. 최신 플랜은 <a href="https://gzxsy.vip/register?aff=eapz">공식 사이트</a>에서 확인할 수 있습니다.
    </td>
  </tr>
</table>

기타 후원자: [Wonderdch](https://github.com/Wonderdch), [suxinwl](https://github.com/suxinwl), [Hermit](https://github.com/HermitChen), [Suifeng023](https://github.com/Suifeng023), [HK-hub](https://github.com/HK-hub)

## 생태계 조합

### OpenCowork

- 저장소: [AIDotNet/OpenCowork](https://github.com/AIDotNet/OpenCowork)
- 추천 조합: OpenCowork 는 로컬 파일 작업, 멀티 Agent 실행, 메시지 플랫폼 연동, 데스크톱 자동화를 맡기고, CodexManager 는 Codex 계정 관리, 사용량 추적, 플랫폼 Key, 로컬 게이트웨이 진입점을 담당하게 구성하는 방식이 잘 맞습니다.
- 적합한 장면: "실행 작업 공간 / 사무 협업"과 "계정 풀 관리 / 게이트웨이 입구"를 분리하고 싶을 때 두 프로젝트가 서로를 잘 보완합니다.
- 한 문장으로 정리하면: **OpenCowork 는 실행과 현장 작업에 가깝고, CodexManager 는 관리와 게이트웨이에 가깝습니다.**

## 빠른 탐색
| 필요한 작업 | 먼저 볼 문서 |
| --- | --- |
| 첫 실행, 배포, Docker, macOS 허용 처리 | [실행 및 배포 가이드](report/실행-및-배포-가이드.md) |
| Codex CLI / ccswitch `auth.json` 및 `config.toml` 구성 | [실행 및 배포 가이드](report/실행-및-배포-가이드.md#ccswitch를-통한-연결) |
| 환경 변수, 데이터베이스, 포트, 프록시, 수신 주소 설정 | [환경변수 및 실행 설정 안내](report/환경변수-및-실행-설정-안내.md) |
| 계정 라우팅, 가져오기 오류, challenge 차단 문제 해결 | [FAQ 및 계정 라우팅 규칙](report/FAQ-및-계정-라우팅-규칙.md) |
| 백그라운드 작업이 계정을 건너뛰거나 비활성화하는 이유 확인 | [백그라운드 작업 계정 건너뛰기 안내](report/백그라운드-작업-계정-건너뛰기-안내.md) |
| 플러그인 센터 최소 연동 | [플러그인 센터 최소 연동 안내](report/플러그인-센터-최소-연동-안내.md) |
| 내부 명령과 연동 지점 확인 | [시스템 내부 인터페이스 총람](report/시스템-내부-인터페이스-총람.md) |
| 로컬 빌드, 패키징, 릴리스 스크립트 | [빌드·릴리스·스크립트 가이드](release/빌드-릴리스-및-스크립트-가이드.md) |

## 디렉터리 구성

### `release/`
릴리스 노트, 롤백 메모, 산출물 설명, 패키징 가이드.

### `report/`
운영 가이드, 문제 해결 메모, 호환성 보고서, FAQ.

## 추천 문서

### 운영
| 문서 | 설명 |
| --- | --- |
| [실행 및 배포 가이드](report/실행-및-배포-가이드.md) | 데스크톱 첫 실행, Service 버전, Docker, macOS 첫 실행 처리 |
| [환경변수 및 실행 설정 안내](report/환경변수-및-실행-설정-안내.md) | 실행 구성, 기본값, 환경변수를 한곳에서 정리 |
| [FAQ 및 계정 라우팅 규칙](report/FAQ-및-계정-라우팅-규칙.md) | 계정 라우팅과 로그 관련 자주 발생하는 문제 |
| [게이트웨이와 Codex 공식 파라미터 비교표](report/게이트웨이와-Codex-공식-파라미터-비교표.md) | 현재 게이트웨이와 공식 Codex 사이의 파라미터 차이 |
| [백그라운드 작업 계정 건너뛰기 안내](report/백그라운드-작업-계정-건너뛰기-안내.md) | 백그라운드 작업이 계정을 건너뛰거나 비활성화하는 이유 |
| [최소 문제 해결 가이드](report/최소-문제해결-가이드.md) | 가장 흔한 시작/중계 문제를 빠르게 점검 |
| [플러그인 센터 최소 연동 안내](report/플러그인-센터-최소-연동-안내.md) | 마켓 접근에 필요한 최소 필드와 인터페이스 |
| [게이트웨이와 Codex 헤더·파라미터 차이](report/게이트웨이와-Codex-헤더-및-파라미터-차이.md) | 현재 게이트웨이와 Codex 간 요청 차이 정리 |
| [플러그인 센터 연동 및 인터페이스 목록](report/플러그인-센터-연동-및-인터페이스-목록.md) | 마켓 모드, RPC/Tauri 명령, 매니페스트 필드, Rhai 인터페이스 |
| [시스템 내부 인터페이스 총람](report/시스템-내부-인터페이스-총람.md) | 내부 명령, RPC 엔드포인트, 플러그인 내장 함수 |

### 빌드와 릴리스
| 문서 | 설명 |
| --- | --- |
| [빌드·릴리스·스크립트 가이드](release/빌드-릴리스-및-스크립트-가이드.md) | 로컬 빌드, 스크립트 파라미터, GitHub workflow |
| [릴리스 및 산출물 안내](release/릴리스-및-산출물-안내.md) | 산출물 이름, 배포 규칙, 릴리스 결과 |
| [스크립트 및 릴리스 책임 매트릭스](report/스크립트-및-릴리스-책임-매트릭스.md) | 어떤 스크립트/워크플로가 어떤 역할을 맡는지 정리 |

## 문서 규칙

### 다음 문서는 커밋할 가치가 있습니다
- 앞으로도 다른 기여자에게 도움이 되는 문서,
- 개발·테스트·배포·문제 해결 방식에 영향을 주는 문서,
- 프로젝트의 장기적인 기준 문서가 되는 내용.

### 다음 문서는 커밋하지 않는 편이 좋습니다
- 임시 초안,
- 개인 작업 메모,
- 일회성 중간 산출물,
- 로컬 전용 실험 기록.

## 무시 패턴
- `docs/**/*.tmp.md`
- `docs/**/*.local.md`

공식 문서에는 위 접미사를 사용하지 마세요.

## 파일 이름 규칙

```text
장기 유지 문서: topic.md
일회성 보고서: yyyyMMddHHmmssfff_topic.md
```

## 유지보수 메모
- 중요한 문서는 README에 계속 추가하지 말고 `docs/` 아래에 두세요.
- 버전 기록은 `변경-이력.md`에서 관리하세요.
- 아키텍처 메모는 `아키텍처.md`에 유지하세요.
- 협업 규칙은 `기여-가이드.md`에 유지하세요.
- 미출시 변경 사항의 상세 내용은 `변경-이력.md`에 적고, README는 탐색과 요약 위주로 유지하세요.

## 연락처
- WeChat: 그룹 참여가 필요하면 `ProsperGao` 를 추가하고, 용건을 함께 알려 주세요
- Telegram 그룹: [CodexManager TG 그룹](https://t.me/+OdpFa9GvjxhjMDhl)
