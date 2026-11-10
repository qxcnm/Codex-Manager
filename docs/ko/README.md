# CodexManager

<table align="center">
  <tr>
    <td align="center" valign="middle" width="44%">
      <img src="../../assets/logo/readme-header-ko.png" alt="CodexManager — 로컬 데스크톱 및 서비스용 Codex 계정 풀 관리자" width="460" />
      <br />
      <sub>
        <a href="https://qxnm.top/">공식 사이트</a> ·
        <a href="#스폰서">스폰서</a>
      </sub>
    </td>
    <td align="center" valign="middle" width="13%">
      <sub>
        <a href="../../README.md">English</a>
        <br />
        <a href="../zh-CN/README.md">中文</a>
        <br />
        <a href="../ru/README.md">Русский</a>
        <br />
        <strong>한국어</strong>
      </sub>
    </td>
    <td align="center" valign="middle" width="23%">
      <a href="https://github.com/qxcnm/Codex-Manager">
        <img alt="GitHub Stars" src="https://img.shields.io/github/stars/qxcnm/Codex-Manager?style=flat&logo=github&label=GitHub" height="20" />
      </a>
      <br />
      <a href="https://atomgit.com/qxnm/Codex-Manager">
        <img alt="AtomGit Stars" src="https://atomgit.com/qxnm/Codex-Manager/star/badge.svg" height="20" />
      </a>
      <br />
      <a href="https://gitee.com/hongshungao/Codex-Manager">
        <img alt="Gitee Stars" src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fgitee.com%2Fapi%2Fv5%2Frepos%2Fhongshungao%2FCodex-Manager&amp;query=%24.stargazers_count&amp;suffix=%20Stars&amp;label=Gitee&amp;logo=gitee&amp;color=C71D23" height="20" />
      </a>
    </td>
    <td align="center" valign="middle" width="20%">
      <sub><strong>인정 커뮤니티</strong></sub>&nbsp;
      <a href="https://linux.do/t/topic/1688401" title="LINUX DO">
        <img src="https://cdn3.ldstatic.com/original/4X/d/1/4/d146c68151340881c884d95e0da4acdf369258c6.png" alt="LINUX DO" height="30" />
      </a>
      &nbsp;
      <a href="https://xuanwu.openatom.org/articles/project/codex-manager/" title="쉬안우 커뮤니티">
        <img src="https://xuanwu.openatom.org/logo.png" alt="쉬안우 커뮤니티" height="30" />
      </a>
    </td>
  </tr>
</table>

CodexManager는 데스크톱 앱과 서비스 모드에서 Codex 계정, 사용량, 플랫폼 Key, 모델 라우팅을 한곳에서 관리합니다. 인터페이스는 영어, 중국어, 러시아어, 한국어를 지원합니다. 새 설치의 기본 **자동 감지** 설정은 브라우저 또는 데스크톱 WebView가 보고하는 언어를 따르며, 지원하지 않거나 언어 정보를 확인할 수 없으면 영어로 표시합니다. 상단 언어 메뉴나 설정에서 언어를 직접 선택할 수 있으며, 저장된 선택은 유지됩니다.

**CodexManager는 [쉬안우 커뮤니티](https://xuanwu.openatom.org/articles/project/codex-manager/)에 합류했습니다.** 이 커뮤니티는 OpenAtom Foundation이 인큐베이팅하고 운영하는 Rust 기술 커뮤니티입니다.

<table>
  <tr>
    <td valign="top" width="50%">
      <strong>소스 코드 안내</strong>
      <br />
      <sub>
        이 제품은 제가 지휘하고 AI의 도움을 받아 제작했습니다: Codex(98%), Gemini(2%). 문제가 발생하면 서로 존중하며 소통해 주세요. 모든 패키지를 검증할 수 있는 환경을 갖추지 못했기 때문에 Windows 데스크톱 버전만 사용 가능성을 보장합니다. 다른 플랫폼의 문제는 충분히 테스트한 뒤 Issue로 제출해 주세요. 플랫폼별 문제를 제보하고 테스트에 참여해 주신 모든 분께 감사드립니다.
      </sub>
    </td>
    <td valign="top" width="50%">
      <strong>면책 조항</strong>
      <br />
      <sub>
        이 프로젝트는 학습과 개발 목적으로만 제공됩니다. 사용자는 OpenAI, Anthropic 등 관련 플랫폼의 서비스 약관을 준수해야 합니다. 작성자는 계정, API Key 또는 프록시 서비스를 제공하거나 배포하지 않으며 소프트웨어의 구체적인 사용 방식에 책임을 지지 않습니다. 속도 제한이나 서비스 제한을 우회하는 용도로 사용하지 마세요.
      </sub>
    </td>
  </tr>
</table>

## 스폰서

이 프로젝트는 스폰서 협업을 받고 있으며 비용은 협의할 수 있습니다. 스폰서는 README와 프로젝트 내 스폰서 영역에 브랜드 로고, 소개, 공식 링크를 표시할 수 있습니다. 문구, 위치, 표시 기간, 게시 시점은 상호 협의하며, 개발자 또는 AI 도구 생태계와 관련되고 관련 법규를 준수해야 합니다.

**협업 문의:** WeChat, Telegram 또는 `18272669457@163.com`으로 연락해 주세요. WeChat 친구 추가 시 “스폰서 협업”이라고 적어 주세요.

CodexManager를 지원해 주신 모든 분과 파트너께 감사드립니다.

[![PatewayAI 공식 고품질 모델 API 중계 서비스](../../assets/images/sponsors/patewayai-banner-en-4k.png)](https://pateway.ai/?ch=kimnmd)

이 프로젝트를 후원해 주신 <strong>PatewayAI</strong>에 감사드립니다!

PatewayAI는 공식 고품질 모델 API 중계에 집중하며 Claude와 Codex 시리즈를 완전히 지원하고, 투명한 과금과 엔터프라이즈급 서비스를 제공합니다.

경제 모드는 표준 요금의 5%부터 시작합니다. <a href="https://pateway.ai/?ch=kimnmd" rel="sponsored nofollow">이 링크</a>로 가입하면 체험 크레딧과 비정기 무료 크레딧 이벤트에 참여할 수 있으며, 양방향 추천 보상은 최대 150달러입니다.

---

<table>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%201"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 1 · 월 900위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%201">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%202"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 2 · 월 800위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%202">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%203"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 3 · 월 700위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%203">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%204"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 4 · 월 600위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%204">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%205"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 5 · 월 500위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%205">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%206"><img src="../../assets/images/sponsors/sponsor-recruitment.svg" alt="스폰서 모집" width="120" /></a>
    </td>
    <td valign="top">
      <strong>스폰서 자리 6 · 월 400위안(CNY) · 모집 중</strong><br />
      CodexManager 개발자 커뮤니티에 브랜드를 소개하세요.<br />
      AI 도구, API 서비스, 개발자 제품의 참여를 환영합니다. 브랜드 로고, 제품 소개, 공식 링크를 표시할 수 있습니다.<br />
      <a href="mailto:18272669457@163.com?subject=CodexManager%20sponsorship%20-%20slot%206">이 자리 문의하기</a> · 18272669457@163.com
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://daitouai.com/"><img src="../../assets/images/sponsors/daitouai.svg" alt="DaitouAI (呆头 AI)" width="120" /></a>
    </td>
    <td valign="top">
      <strong><a href="https://daitouai.com/">DaitouAI (呆头 AI)</a></strong>는 중국 주요 대규모 모델 API 서비스를 전문으로 하며 GLM, Kimi, DeepSeek 등에 간편하게 연결할 수 있습니다. 대화와 질의응답, 코드 생성, 콘텐츠 제작, 자동화 개발에 적합하며 공식 가격보다 저렴한 요금을 제공합니다. 개인 개발자의 빠른 체험부터 독립 프로젝트와 팀의 필요에 따른 호출까지 중국 모델 사용 비용을 낮출 수 있습니다. 사용 가능한 모델, 연결 방법, 최신 혜택은 <a href="https://daitouai.com/">공식 사이트</a>에서 확인하세요.
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://www.aixiamo.com/?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager&utm_content=logo_brand_home_v3"><img src="../../assets/images/sponsors/aixiamo.jpg" alt="AI夏末 AIXiamo" width="120" /></a>
    </td>
    <td valign="top">
      <strong><a href="https://www.aixiamo.com/?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager&utm_content=brand_home_v3">AIXiamo 공식 사이트 | 중국 내 본인 계정 ChatGPT Plus / Pro 정식 충전</a></strong><br />
      <strong>Codex 한도가 부족하다면 Plus와 Pro 중 무엇을 선택할까요?</strong> 일상 사용은 <a href="https://www.aixiamo.com/chatgpt-plus-domestic-recharge?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager&utm_content=plus_owner_v3">Plus</a>, 한도에 자주 도달하거나 장시간 작업은 <a href="https://www.aixiamo.com/chatgpt-pro?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager&utm_content=pro_owner_v3">Pro 5x / 20x</a>를 비교하세요.<br />
      <strong>스폰서 가이드:</strong> <a rel="sponsored nofollow" href="https://gpt-guide.aixiamo.com/knowledge">2026 중국 ChatGPT Plus / Pro 충전 및 요금제 선택 가이드</a><br />
      해외 은행 카드 없이 Alipay를 지원하며 비밀번호, 인증 코드, 복구 코드를 요구하지 않습니다. 주문 조회와 중국어 상담을 제공하고, 충전 실패 시 확인 후 전액 환불합니다. 3년 이상 운영하면서 온라인과 오프라인으로 수만 명의 사용자에게 서비스를 제공했으며, Claude Pro / Max, Gemini(Google AI Pro), Grok / SuperGrok 등 주요 AI 멤버십도 제공합니다.<br />
      기업 구매도 가능하며, <a href="https://www.aixiamo.com/articles/aixiamo-invoice-application-notice-2026?utm_source=github&utm_medium=sponsor&utm_campaign=codex_manager&utm_content=invoice_notice_v3">완료된 주문은 상담 확인 후 청구서를 신청</a>할 수 있습니다.
    </td>
  </tr>
  <tr>
    <td align="center" valign="middle" width="180">
      <a href="https://gzxsy.vip/register?aff=eapz"><img src="../../assets/images/sponsors/xingsiyan.jpg" alt="Xing Si Yan Gateway" width="120" /></a>
    </td>
    <td valign="top">
      <strong>Xing Si Yan Gateway</strong>는 Claude Code, Codex 등 모델 호출을 위한 안정적인 중계와 지원 서비스를 제공합니다. 고가용성 API, 간편한 도입, 지속적인 제공 지원이 필요한 개발자와 팀에 적합합니다. 최신 상품은 <a href="https://gzxsy.vip/register?aff=eapz">공식 사이트</a>에서 확인하세요.
    </td>
  </tr>
</table>

그 밖의 후원자: 呆头呆脑, [Wonderdch](https://github.com/Wonderdch), [suxinwl](https://github.com/suxinwl), [Hermit](https://github.com/HermitChen), [Suifeng023](https://github.com/Suifeng023), [HK-hub](https://github.com/HK-hub)

## ☕ 프로젝트 후원

이 프로젝트가 도움이 되었다면 후원해 주세요!

<table>
  <tr>
    <th>Alipay</th>
    <th>WeChat Pay</th>
  </tr>
  <tr>
    <td align="center"><img src="../../assets/images/AliPay.jpg" alt="Alipay 후원 QR 코드" width="220" /></td>
    <td align="center"><img src="../../assets/images/wechatPay.jpg" alt="WeChat Pay 후원 QR 코드" width="220" /></td>
  </tr>
</table>

## Star History

<a href="https://www.star-history.com/?repos=qxcnm%2FCodex-Manager&amp;type=date&amp;legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=qxcnm%2FCodex-Manager&amp;type=date&amp;legend=top-left&amp;theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=qxcnm%2FCodex-Manager&amp;type=date&amp;legend=top-left" />
   <img alt="Codex Manager Star History Chart" src="https://api.star-history.com/chart?repos=qxcnm%2FCodex-Manager&amp;type=date&amp;legend=top-left" />
 </picture>
</a>

## 홈 탐색

| 하려는 작업 | 바로가기 |
| --- | --- |
| 최초 실행, 배포, Docker, macOS 허용 설정 | [실행 및 배포 가이드](report/실행-및-배포-가이드.md) |
| Codex CLI / ccswitch, `auth.json`, `config.toml` 설정 | [실행 및 배포 가이드](report/실행-및-배포-가이드.md#ccswitch를-통한-연결) |
| Codex 로그인 없이 ChatGPT `/api/auth/session`으로 계정 가져오기 | [중국어 ChatGPT session 가져오기 가이드](../zh-CN/report/不登陆Codex使用ChatGPT-auth-session导入账号.md) |
| 포트, 프록시, 데이터베이스, Web 암호, 환경변수 설정 | [환경변수 및 실행 설정](report/환경변수-및-실행-설정-안내.md) |
| 계정 라우팅, 가져오기 실패, challenge, 요청 오류 해결 | [FAQ 및 계정 라우팅 규칙](report/FAQ-및-계정-라우팅-규칙.md) |
| 백그라운드 작업이 계정을 건너뛰거나 비활성화하는 이유 | [백그라운드 작업 계정 안내](report/백그라운드-작업-계정-건너뛰기-안내.md) |
| 모델, 가격, 라우트, instructions policy, Codex 모델 카탈로그 적용 | [중국어 Model Catalog V2 가이드](../zh-CN/report/模型目录V2管理与计费说明.md) |
| 플러그인 센터 최소 연동 | [플러그인 센터 최소 연동 안내](report/플러그인-센터-최소-연동-안내.md) |
| 플러그인 인터페이스, marketplace 모드, Rhai API | [플러그인 센터 연동 및 인터페이스](report/플러그인-센터-연동-및-인터페이스-목록.md) |
| 전체 시스템 내부 인터페이스 | [시스템 내부 인터페이스 총람](report/시스템-내부-인터페이스-총람.md) |
| 로컬 빌드, 패키징, 릴리스, 스크립트 | [빌드·릴리스·스크립트 가이드](release/빌드-릴리스-및-스크립트-가이드.md) |

## 기능 개요

- 계정 풀 관리: 그룹, 태그, 정렬, 메모, 차단 감지와 필터링.
- 일괄 가져오기/내보내기: 여러 파일, 데스크톱 JSON 폴더 재귀 가져오기, 계정별 단일 파일 내보내기.
- 사용량 표시: 표준 5시간 + 7일 창, 7일 전용 계정, Code Review / Spark 등의 추가 한도를 남은 비율과 초기화 시간으로 통합 표시.
- [한도 초기화 후 자동 깨우기(중국어)](../zh-CN/额度重置自动唤醒.md): 소진된 5시간 한도의 초기화 시각이 되면 짧은 준비 메시지를 기본으로 전송합니다. 계정별 또는 일괄 활성화·비활성화를 지원하여 다음 한도 새로고침을 기다리는 시간을 줄입니다.
- 계정 인증: `chatgpt.com` 브라우저 OAuth와 Device Code 로그인, 콜백 URL 수동 붙여넣기.
- 플랫폼 Key: 임의 생성 또는 고정 Key 지정, 비활성화, 삭제, 모델 바인딩, 추론 등급, 서비스 등급(요청 따르기 / Standard / Fast / Ultrafast / Flex)을 지원합니다. 사용자 지정 계정 그룹에 바인딩할 수 있으며, 계정 플랜 필터와의 교집합으로 허용된 풀 안에서만 순환합니다.
- 모델 관리: Model Catalog V2를 유일한 런타임 기준으로 사용하며 builtin/custom, 정수로 저장하는 세 가지 가격과 긴 컨텍스트 단계별 가격, 계정 풀/집계 API 라우트, instructions policy, 로컬 JSON preview/commit을 지원합니다. 모델을 선택한 뒤 Codex의 `model_catalog_json`에 적용할 수 있습니다.
- 현재 builtin 카탈로그에는 12개 레코드가 있습니다. `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, GPT-5.6 세 모델, `gpt-5.5`, `gpt-image-2`, Image 2.5 변형 두 개까지 총 11개 모델이 기본 표시되며, `codex-auto-review`만 숨겨집니다.
- `gpt-5.4`와 `gpt-5.4-mini`는 2026-08-31에 ChatGPT 로그인 방식의 Codex에서 retired되었고, `gpt-5.2`도 해당 로그인 경로에서 deprecated되었습니다. 일반 API 모델은 이 정리의 영향을 받지 않습니다. revision 9는 수정되지 않은 해당 builtin을 제거하지만 사용자 편집, 가격 tier, 기본값이 아닌 route, permission/API Key 연결은 custom 모델로 전환해 보존합니다. API Key 사용자는 같은 slug를 custom 모델로 명시적으로 추가할 수도 있습니다.
- 집계 API: 최소 중계 기능의 외부 업스트림을 관리하며 생성, 편집, 잔액 조회, 구성된 V2 라우트 기준의 연결 테스트를 제공합니다. 관리자가 모델을 직접 가져와 선택적으로 연결하거나, 계정 또는 연결별로 [백그라운드 모델 동기화](../en/report/model-discovery-background.md)를 시작하고 중지할 수 있습니다. 기본값은 꺼짐이며, 기존 설정을 유지하면서 누락된 모델과 라우트만 추가합니다.
- 플러그인 센터: `/plugins/`에서 내장 추천, 기업 비공개, 사용자 소스 marketplace와 manifest, 작업, 로그, Rhai 인터페이스를 제공.
- Skills와 플러그인: `/skills/`에서 Skills 설치와 Codex 플러그인 설치를 별도 탭으로 제공합니다. 내장 및 사용자 지정 GitHub 스킬 저장소, 저장소 새로고침과 개별 Skill 설치, skills.sh 검색·설치, ZIP/폴더 가져오기, 설치 항목 관리를 지원합니다. Codex 플러그인 설치는 기본 Marketplace의 전체 설치 과정을 유지하며 `.system` 내장 Skill은 항상 읽기 전용입니다. 서비스를 실행하는 사용자의 `~/.agents/skills`도 읽기 전용으로 표시하며, 설치와 제거는 기존 관리 스킬 디렉터리로 제한됩니다.
- 프로젝트 실행: 데스크톱에서 로컬 폴더를 즐겨찾기하고 Windows/macOS의 ChatGPT Codex App으로 열며, “세션”은 로컬 프로필로 새 터미널의 `resume` 선택기를 실행.
- 설정: “시스템 기준 계산” 버튼, 계정별 동시 요청 상한, 업스트림 프록시, 요청 전체 시간 제한, 스트림 유휴 시간 제한, SSE 연결 유지 스위치와 주기, 더 보수적인 고동시성 성능 저하 정책을 제공합니다. SSE 연결 유지는 기본으로 켜지며 `CODEXMANAGER_SSE_KEEPALIVE_ENABLED=0`(또는 `false`)으로 끌 수 있습니다. 실험적 업스트림 WebSocket은 기본으로 꺼져 있으며 `CODEXMANAGER_USE_WEBSOCKET_UPSTREAM=1`로 켤 수 있습니다.
- 시스템 내부 인터페이스 목록: 데스크톱/서비스 명령, RPC 메서드, 플러그인 내장 함수.
- 로컬 서비스: 자동 시작, 포트와 수신 주소 설정.
- 로컬 게이트웨이: Codex CLI, Gemini CLI, Claude Code와 타사 도구를 위한 OpenAI 호환 엔드포인트; Gemini → `/v1/responses`, SSE, tools, MCP, skills, 요청/스트림 timeout 지원.
- 이미지 생성: 공식 Codex 동작에 따라 `/v1/responses`에 `image_generation` tool을 기본 주입하며, 명시적 tool 전달과 `/v1/images/generations`, `/v1/images/edits` 호환 엔드포인트를 지원합니다. Images API 호환 메인 모델의 기본값은 `gpt-6-luna`이며, 이미지 tool의 기본 모델은 `gpt-image-2`이고 Image 2.5 두 변형은 명시적으로 선택할 수 있습니다.

## 스크린샷

![대시보드](../../assets/images/dashboard%20.png)

![OpenAI 계정 풀 (목록 보기)](../../assets/images/accounts.png)

![OpenAI 계정 풀 (카드 보기)](../../assets/images/accounts2.png)

![집계 API](../../assets/images/aggregate-api%20.png)

![플랫폼 키](../../assets/images/platform-key%20.png)

![모델 및 라우팅](../../assets/images/models.png)

![요청 로그](../../assets/images/log.png)

![시스템 설정](../../assets/images/setting.png)

![플러그인 센터](../../assets/images/plug.png)

## 빠른 시작

1. [GitHub Releases](https://github.com/qxcnm/Codex-Manager/releases/latest)에서 사용 중인 플랫폼의 패키지를 다운로드한 뒤 데스크톱 앱을 실행하고 **서비스 시작**을 클릭합니다.
2. **계정 관리**에서 브라우저 인증 또는 Device Code를 선택해 `chatgpt.com`에 로그인합니다.
3. 브라우저 콜백이 실패하면 콜백 URL을 붙여넣어 수동으로 처리합니다.
4. 사용량을 새로고침하고 계정 상태를 확인합니다.

## 기본 데이터 디렉터리

- 데스크톱 앱의 SQLite 데이터베이스는 앱 데이터 디렉터리에 `codexmanager.db`로 저장됩니다.
- Windows: `%APPDATA%\\com.codexmanager.desktop\\codexmanager.db`
- macOS: `~/Library/Application Support/com.codexmanager.desktop/codexmanager.db`
- Linux: `~/.local/share/com.codexmanager.desktop/codexmanager.db`
- 업그레이드 후 첫 초기화 전에 같은 디렉터리에 `codexmanager.db.pre-<버전>.bak`을 남기며 같은 버전의 재시도는 덮어쓰지 않습니다.
- 설정의 “데스크톱 진단”에서 Debug 모드, 일반 파일 로그 비활성화, 로그 폴더 열기를 사용할 수 있습니다. 일반 로그는 512KB로 제한되며 요청 로그와 Token/비용 통계는 영향을 받지 않습니다.
- UI가 시작되지 않으면 `CodexManager.exe --debug`(macOS/Linux는 `CodexManager --debug`)를 사용하세요. 오류 이유를 표시하고 안내된 디렉터리에 `startup-error.log`를 기록합니다.
- 데이터베이스, 프록시, 수신 주소 등은 [환경변수 및 실행 설정](report/환경변수-및-실행-설정-안내.md)을 참고하세요.
- Service는 해당 드라이버를 포함해 빌드하면 SeaORM MySQL/PostgreSQL 백엔드도 사용할 수 있습니다. 기존 SQLite는 읽기 전용으로 내보낸 뒤 빈 대상 데이터베이스에 트랜잭션으로 가져올 수 있습니다. 자세한 내용은 [데이터베이스 전환 및 오프라인 가져오기(중국어)](../zh-CN/report/环境变量与运行配置说明.md#service-数据库切换与离线导入)를 참고하세요. 로컬 격리 데이터베이스 검증은 운영 환경 검증을 의미하지 않으며, 현재 범위는 [마이그레이션 검증 기록(중국어)](../zh-CN/report/Axum-Tokio-Tower-SeaORM迁移进度与会话交接.md)에서 확인할 수 있습니다.
- Docker 기본값은 `TZ=Asia/Shanghai`이며 Compose는 환경의 `TZ`를 우선 사용합니다. 다른 지역에서는 적절한 IANA 시간대를 설정하세요.

## 화면 안내

### 데스크톱

- 계정 관리: 계정과 사용량 가져오기, 내보내기, 새로고침, 낮은 한도/차단 필터와 초기화 시간.
- 플랫폼 Key: 모델, 추론 등급, 서비스 등급별 Key 바인딩과 호출 로그.
- Codex 연결 방식: OpenAI 계정 직접 연결, Responses에 안전하게 매핑할 수 있는 집계 API 직접 연결, CodexManager 로컬 게이트웨이 연결을 지원합니다. 계정 직접 연결은 계정을 전환하기 위해 `auth.json`을 기록합니다. 집계 API 직접 연결과 게이트웨이는 `auth.json`을 유지하고 `config.toml`만 업데이트하며, provider의 `experimental_bearer_token`에는 집계 API 직접 연결 시 선택한 집계 API Key를, 게이트웨이 연결 시 CodexManager 플랫폼 Key를 기록합니다. 집계 API 직접 연결은 로컬 게이트웨이를 거치지 않으므로 게이트웨이 요청 로그에 기록되지 않습니다.
- 모델 관리: 데스크톱/Web은 `~/.codex/models_cache.json`을 기록하거나 다운로드하지 않습니다. 직접 계정은 공식 카탈로그, 로컬 게이트웨이는 독립 관리 카탈로그를 사용합니다.
- 로컬 게이트웨이는 기존 ChatGPT 또는 API Key 인증(`auth_mode = "chatgpt"` 또는 `"apikey"`)을 지원하며 특정 로그인 방식을 요구하지 않습니다. 게이트웨이 설정은 `config.toml`만 업데이트하고 기존 `auth.json`을 바이트 단위로 유지합니다. 인증 파일을 생성하거나 ChatGPT 로그인 토큰을 요구하지 않습니다. `cm` provider는 `name = "cm"`, `requires_openai_auth = true`를 사용하며 플랫폼 Key는 `experimental_bearer_token`에 기록합니다.
- 플러그인 센터: `/plugins/` marketplace 전환, 설치·활성화, 작업, 로그, Rhai.
- Skills와 플러그인: `/skills/` 별도 탭, GitHub/skills.sh 설치, ZIP/폴더 가져오기, 안전한 제거, Marketplace, 읽기 전용 시스템 Skills. 서비스를 실행하는 사용자의 `~/.agents/skills`도 읽기 전용으로 표시하며, 설치와 제거는 기존 관리 스킬 디렉터리로 제한됩니다.
- 프로젝트 실행: 데스크톱은 로컬 폴더를 저장하고 Windows/macOS는 ChatGPT Codex App으로 엽니다. 세션은 로컬 CLI를 사용하며 Web/Docker는 장치 폴더에 접근하지 않습니다.
- 설정: 포트, 수신 주소, 프록시, timeout, SSE keepalive, 테마, 업데이트, 백그라운드 동작.

### Service 버전

- `codexmanager-service`: 로컬 OpenAI 호환 게이트웨이.
- `codexmanager-web`: 브라우저 관리 화면과 `/api/runtime`, `/api/rpc` 프록시.
- `codexmanager-start`: service + web 동시 시작.

## 주요 문서

- 변경 이력: [변경 이력](변경-이력.md)
- 기여 가이드: [기여 가이드](기여-가이드.md)
- 아키텍처: [아키텍처](아키텍처.md)
- 테스트: [테스트](테스트.md)
- 보안: [보안](보안.md)
- Model Catalog V2: [중국어 가이드](../zh-CN/report/模型目录V2管理与计费说明.md)
- 문서 인덱스: [중국어 문서 인덱스](../zh-CN/README.md#文档首页)

## 주제별 문서

| 문서 | 내용 |
| --- | --- |
| [실행 및 배포 가이드](report/실행-및-배포-가이드.md) | 최초 실행, Docker, Service 버전, macOS 허용 |
| [환경변수 및 실행 설정](report/환경변수-및-실행-설정-안내.md) | 앱 설정, 프록시, 주소, 데이터베이스, Web 보안 |
| [FAQ 및 계정 라우팅 규칙](report/FAQ-및-계정-라우팅-규칙.md) | 계정 선택, challenge, 가져오기/내보내기, 오류 |
| [중국어 ChatGPT session 가이드](../zh-CN/report/不登陆Codex使用ChatGPT-auth-session导入账号.md) | session JSON 복사와 일괄 가져오기 |
| [백그라운드 계정 안내](report/백그라운드-작업-계정-건너뛰기-안내.md) | 백그라운드 필터, 비활성 계정과 workspace 상태 |
| [최소 문제 해결 가이드](report/최소-문제해결-가이드.md) | 서비스 시작, 요청 중계, 모델 새로고침 |
| [플러그인 센터 연동](report/플러그인-센터-연동-및-인터페이스-목록.md) | 라우트, marketplace, Tauri/RPC, manifest, Rhai |
| [빌드·릴리스·스크립트](release/빌드-릴리스-및-스크립트-가이드.md) | 로컬 빌드, Tauri, workflow, 스크립트 |
| [릴리스 및 산출물](release/릴리스-및-산출물-안내.md) | 플랫폼별 산출물, 이름, pre-release |
| [스크립트·릴리스 책임](report/스크립트-및-릴리스-책임-매트릭스.md) | 스크립트 역할과 사용 시점 |
| [게이트웨이와 Codex 차이](report/게이트웨이와-Codex-헤더-및-파라미터-차이.md) | 요청 헤더와 파라미터 비교 |
| [시스템 내부 인터페이스](report/시스템-내부-인터페이스-총람.md) | 데스크톱, 서비스, 플러그인 센터 인터페이스 |
| [변경 이력](변경-이력.md) | 릴리스, 미출시 변경, 전체 이력 |

## 디렉터리 구조

```text
.
├─ apps/                # 프런트엔드와 Tauri 데스크톱
│  ├─ src/
│  ├─ src-tauri/
│  └─ out/
├─ crates/              # Rust core/service
│  ├─ core
│  ├─ service
│  ├─ start              # service + web 시작
│  └─ web                # Service Web UI와 /api/rpc 프록시
├─ docs/                # 공식 문서
├─ scripts/             # 빌드·릴리스 스크립트
└─ README.md
```

## 감사 및 참고 프로젝트

- Codex(OpenAI): 요청 흐름, 로그인 의미, upstream 호환 구현과 소스 구조를 참고했습니다. <https://github.com/openai/codex>
- CLIProxyAPI(CPA): Responses 요청 변환과 tool call 규칙을 참고했습니다. <https://github.com/router-for-me/CLIProxyAPI>

## 연락처

- WeChat 공식 계정: 七线牛马
- WeChat: 그룹 참여는 `ProsperGao`를 추가하고 목적을 알려 주세요.
- Telegram: [CodexManager TG 그룹](https://t.me/+OdpFa9GvjxhjMDhl)
