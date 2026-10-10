# 현재 게이트웨이와 Codex 매개변수 전달 비교표

참고: 현재 작업 공간에서는 Codex 아웃바운드 직접 계약 차이가 수집되었습니다. 다음은 "파라미터 이동 방법/최종적으로 역에서 나가는 방법/상류와 일치 여부"에 따라 재구성됩니다.

## 1. 매개변수 전송 링크

1. 인바운드 HTTP 요청은 먼저 `crates/service/src/gateway/request/incoming_headers.rs`에 진입합니다. 여기서는 헤더 스냅샷만 찍히고 요청은 직접 다시 작성되지 않습니다.
2. 세션 선호도는 `crates/service/src/gateway/request/session_affinity.rs`에 의해 균일하게 계산되어 `incoming_session_id`, `incoming_client_request_id` 및 `fallback_session_id`을 산출합니다.
3. 요청 본문은 `crates/service/src/gateway/request/request_rewrite.rs`에서 재작성 프로세스에 들어간 후 `official_responses_http.rs`에서 Responses 호환성 필드가 처리됩니다.
4. 최종 아웃바운드 헤더는 `crates/service/src/gateway/upstream/headers/codex_headers.rs`에 의해 구성됩니다.
5. 실제로 업스트림으로 보내기 전에 `crates/service/src/gateway/upstream/attempt_flow/transport.rs`에 따라 헤더 + 본문이 reqwest에 전달됩니다.

## 2. 요청 헤더 비교

| 필드 | 현재 업로드 방법 | Codex 업스트림 | 상태 | 비고 |
| --- | --- | --- | --- | --- |
| `Authorization` | 현재 계정 토큰에서 `Bearer <현재 계정 토큰>` | Bearer 토큰도 사용 | 정렬 | 로그인/계정 링크에서 제공되는 토큰, 아웃바운드 시 현재 계정 값으로 대체됨 |
| `originator` | 직접 보내기 `codex_cli_rs` | 동일한 `codex_cli_rs` 보내기 | 정렬 | 런타임 구성 가능 원래 값은 아웃바운드 헤더 |
| `User-Agent` | `codex_cli_rs/<런타임 버전> (<os/version; arch>) <terminal>` | `codex_cli_rs/<DB 설정 버전> (<os/version; arch>) <terminal>` | 구현 수준의 차이점 | 버전 소스가 다르며 형식이 정렬됩니다 |
| `x-client-request-id` | `conversation_id`은 세션 어피니티 링크에 의해 먼저 취해지며, 그렇지 않은 경우 보완되지 않습니다 | 업스트림은 `conversation_id`을 요청 ID로 사용 | 정렬 | 장애 조치 중에 새로운 가치가 창출되지 않습니다 |
| `session_id` | `conversation_id`/폴백 세션에서 계산되며 장애 조치 중에 폴백으로 전환됩니다 | 업스트림은 주로 `conversation_id` | 정렬 | 현재 구현에서는 선호도/대체 전략 |
| `x-openai-subagent` | 현재 요청의 하위 에이전트를 투명하게 전송 | 업스트림은 또한 동일한 의미를 가진 하위 에이전트를 전달합니다 | 정렬 | 값이 있는 경우에만 전송됩니다 |
| `x-codex-beta-features` | 투명한 인바운드 가치 | 업스트림에서 전송됩니다 | 정렬 | 값이 있는 경우에만 보내기 |
| `x-codex-turn-metadata` | 투명한 인바운드 가치 | 업스트림 HTTP 아웃바운드도 수행됩니다. WS 측도 `client_metadata` | 정렬 | 현재 게이트웨이는 동일한 WS 모델을 추가로 구성하지 않습니다. `client_metadata` |
| `x-codex-turn-state` | 직접 연결되면 유지되고 `strip_session_affinity` | 업스트림도 이 세션 상태에 따라 달라집니다. | 정렬 | 선호도가 제거되지 않은 경우에만 전송됩니다 |
| `OpenAI-Beta` | 스트리밍 요청이 전송되었습니다. `responses_websockets=2026-02-06` | 업스트림 WebSocket 경로의 값은 동일합니다. | 정렬 | 스트리밍 요청이 전송되지 않습니다 |
| `x-responsesapi-include-timing-metrics` | 활성화되면 스트리밍 및 전송됨 `true` | 업스트림은 동일한 의미를 갖습니다 | 정렬 | 스트리밍하지 않을 때는 전송되지 않습니다 |
| `ChatGPT-Account-ID` | 더 이상 Codex 아웃바운드 헤더로 전송되지 않음 | 업스트림 직접 요청에서 Codex 프로토콜 헤더로 전송되지 않음 | 정렬 | 내부 계정/사용 경로에만 보관되며 아웃바운드 계약에는 포함되지 않습니다 |

## 3. 요청 매개변수 비교

| 필드 | 현재 전송 방법 | Codex 업스트림 | 상태 | 비고 |
| --- | --- | --- | --- | --- |
| `model` | 인바운드 요청 또는 적응 계층에 의해 선택 후 다시 작성됨 | 프롬프트/모델 선택에 따라 결정되는 업스트림 | 정렬 | 결과 값은 다양한 소스에서 정렬됩니다 |
| `instructions` | 누락된 경우 Codex 호환 경로에 빈 문자열 채우기 | 프롬프트 기본 지시문에 의해 생성된 업스트림 | 정렬 | compat path는 업스트림 검증을 안정화하기 위해 빈 지시문을 유지합니다. |
| `input` | 문자열/객체는 먼저 배열로 정규화됩니다 | 업스트림은 응답 입력 배열을 사용합니다 | 정렬 | 이전 입력 매개변수 형식과 호환 가능 |
| `tools` | 동적 도구는 먼저 `function` 도구에 매핑됩니다 | 업스트림은 프롬프트 도구에서 직접 구축됩니다 | 정렬 | 이름 단축 및 복구 논리는 적응 계층에서 처리됩니다. |
| `tool_choice` | 누락되거나 자동이 아닌 경우 `auto`로 수렴 | 업스트림 코어 경로의 기본값은 `auto` | 정렬 | 프로젝트 확장 값이 더 이상 유지되지 않습니다. |
| `parallel_tool_calls` | 도구 없음 및 기본 시간 보상 `false` | 업스트림은 프롬프트 구성에 의해 명시적으로 전달됩니다. | 부분적으로 일관성 | 결과 필드가 정렬되고 기본 소스가 다릅니다 |
| `reasoning` | 출력은 요청 측 `reasoning` / `reasoning_effort` 및 모델 기본값 | 업스트림은 모델 기본 추론 + 회전 구성에 의해 구축됩니다 | 정렬 | 출력 형태는 일관되지만 소스 링크는 다릅니다 |
| `store` | Codex compat 경로가 `false` | 업스트림이 Azure 응답이 아닌 경우 `false` | 정렬 | Azure 특수 사례는 현재 이 아웃바운드 경로에 포함되어 있지 않습니다. |
| `stream` | 표준 응답 경로가 강제로 적용됨 `true`, 압축 경로는 압축 상태로 유지됨 | 업스트림 응답 기본 경로가 고정됨 `true` | 정렬 | `stream_passthrough` 내부 스위치만 |
| `include` | `reasoning`이 존재할 때만 패딩됨 `["reasoning.encrypted_content"]` | 업스트림은 추론이 존재할 때만 패딩됩니다 | 정렬 | 더 이상 패딩된 배열이 아닙니다 |
| `service_tier` | `Fast -> priority`, 그 외 값은 그대로 유지 | 업스트림도 열거 의미에 따라 매핑됩니다 | 정렬 | 현재 Codex 호환 값만 보고 있습니다 ​​|
| `prompt_cache_key` | 세션 앵커에 의해 생성되며 필요한 경우 내부적으로 강제로 재정의할 수 있습니다 | 직접 사용되는 업스트림 `conversation_id` | 정렬 | 현재 구현은 재정의 기능을 유지하지만 아웃바운드 결과는 정렬됩니다. |
| `text` | verbosity/response_format/schema별 정규화 후 출력 | `create_text_param_for_request()`에 의해 생성된 업스트림 | 정렬 | 출력 구조는 일관됩니다 |
| `stream_passthrough` | 내부 적응 표시로만 사용되며 아웃바운드 전에 제거됩니다 | 업스트림에는 이 필드가 없습니다 | 내부분야 | Codex 프로토콜 차이에는 포함되지 않음 |

## 4. 결론

1. 현재 게이트웨이의 Codex 아웃바운드 요청 헤더 및 요청 매개변수는 업스트림과 동일한 형태를 유지하고 있습니다.
2. 이제 남은 것은 주로 WS 시나리오에서 `User-Agent`의 버전 소스와 `x-codex-turn-metadata`의 `client_metadata` 패키징과 같은 구현 수준의 차이입니다.
3. `stream_passthrough`, 내부 계정 식별, 세션 선호도 폴백 등은 모두 적응 계층의 내부 논리에 속하며 더 이상 계약 차이에 대한 Codex 직접 요청에 포함되지 않습니다.

## 5. 소스코드 기반

- `crates/service/src/gateway/request/incoming_headers.rs`
- `crates/service/src/gateway/request/session_affinity.rs`
- `crates/service/src/gateway/request/request_rewrite.rs`
- `crates/service/src/gateway/request/official_responses_http.rs`
- `crates/service/src/gateway/upstream/headers/codex_headers.rs`
- `crates/service/src/gateway/upstream/attempt_flow/transport.rs`
- `crates/service/src/gateway/core/runtime_config.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\core\src\client.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\common.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\endpoint\responses.rs`
- `D:\MyComputer\own\GPTTeam相关\codex\codex\codex-rs\codex-api\src\requests\headers.rs`
