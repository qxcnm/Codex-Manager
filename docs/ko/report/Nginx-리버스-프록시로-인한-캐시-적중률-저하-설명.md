# Nginx 리버스 프록시로 인한 캐시 적중률 저하 설명

## 결론

CodexManager 를 Docker 또는 독립 실행 Service 앞단에 두고, 그 위에 기본 설정의 Nginx 리버스 프록시를 올리면 가장 흔한 숨은 문제는 "모델 캐시가 고장났다"가 아닙니다. 실제 원인은 **Nginx 가 밑줄이 포함된 커스텀 헤더를 버리는 것**입니다.

CodexManager 는 안정적인 스레드 앵커를 유지하기 위해 다음 헤더에 의존합니다.

- `conversation_id`
- `session_id`
- `x-client-request-id`
- `x-codex-turn-metadata`
- `x-codex-turn-state`

이 헤더가 프록시 계층에서 사라지면 백엔드는 같은 대화를 같은 스레드 앵커에 묶지 못하고, 결과적으로 배포 환경의 `cached_input_tokens` 가 데스크톱 앱보다 훨씬 낮게 보입니다.

## 왜 이런 일이 생기나

기본 Nginx 는 밑줄이 들어간 헤더를 잘 다루지 않습니다.

다음 두 설정을 명시적으로 켜지 않으면:

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

`conversation_id`, `session_id` 같은 헤더가 무효 헤더로 취급되어 CodexManager 에 도달하기 전에 버려질 수 있습니다.

이는 다음 경로에 직접 영향을 줍니다.

1. `crates/service/src/gateway/request/incoming_headers.rs`
   `conversation_id`, `session_id`, `x-codex-turn-state` 등을 추출합니다.
2. `crates/service/src/gateway/request/session_affinity.rs`
   실제 스레드 앵커를 계산합니다.
3. `crates/service/src/gateway/request/official_responses_http.rs`
   안정적인 `prompt_cache_key` 를 요청 본문에 다시 씁니다.

프록시가 이 헤더를 제거하면 백엔드는 약한 fallback 세션 앵커로 퇴화하고, 프롬프트 캐시 재사용도 불안정해집니다.

## 전형적인 증상

- 배포된 Web UI 의 `cached_input_tokens` 가 몇 천 수준에 머묾
- 로컬 데스크톱 앱은 비슷한 요청에서 수만 단위까지 올라감
- 같은 대화에서 여러 번 요청해도 캐시 증가가 크지 않음
- 모델, 계정, 프롬프트가 비슷한데도 배포 결과가 데스크톱보다 유난히 낮음

주의:

- 첫 요청의 캐시 수치가 낮은 것은 정상입니다
- 모델, 계정, 대화가 다르면 캐시 값이 완전히 같을 수는 없습니다
- 진짜 경고 신호는 같은 대화에서 여러 번 요청해도 계속 큰 격차가 유지되는 경우입니다

## 올바른 수정 방법

### 1. 밑줄 헤더 허용

`http {}` 안에 다음을 추가합니다.

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

### 2. 세션 관련 헤더를 명시적으로 전달

API 리버스 프록시 `location` 안에 다음을 추가합니다.

```nginx
proxy_set_header conversation_id $http_conversation_id;
proxy_set_header session_id $http_session_id;
proxy_set_header x-client-request-id $http_x_client_request_id;
proxy_set_header x-openai-subagent $http_x_openai_subagent;
proxy_set_header x-codex-beta-features $http_x_codex_beta_features;
proxy_set_header x-codex-turn-metadata $http_x_codex_turn_metadata;
proxy_set_header x-codex-turn-state $http_x_codex_turn_state;
```

이 설정은 선택 사항이 아니라 배포 기본값으로 보는 편이 안전합니다.

### 3. 스트리밍 API 에서 프록시 버퍼링 끄기

권장 설정:

```nginx
proxy_buffering off;
proxy_request_buffering off;
proxy_read_timeout 3600s;
proxy_send_timeout 3600s;
```

이 설정이 캐시 적중을 직접 만들어 주는 것은 아니지만, Responses, SSE, WebSocket 트래픽에서 프록시 간섭을 줄여 줍니다.

### 4. `/v1/images/` 이미지 생성 경로를 별도 보수 설정으로 처리

CodexManager 는 `/v1/images/generations` 및 `/v1/images/edits` 호환 엔드포인트를 지원합니다. 이 경로는 일반 텍스트 요청과 프록시 위험이 다릅니다.

- `/v1/images/edits` 는 multipart 이미지 업로드로 request body 가 커질 수 있음
- 이미지 생성은 일반 텍스트 첫 토큰보다 오래 걸릴 수 있음
- `b64_json` 응답은 클 수 있어 기본 프록시 버퍼링이 지연 또는 중간 절단 위험을 키울 수 있음

`/v1/images/` 에는 최소한 다음 설정을 유지하는 것이 좋습니다.

```nginx
client_max_body_size 0;
proxy_buffering off;
proxy_request_buffering off;
gzip off;
add_header X-Accel-Buffering no;
proxy_read_timeout 3600s;
proxy_send_timeout 3600s;
send_timeout 3600s;
```

현재 저장소의 `docker/nginx/nginx.conf`는 direct service와 Web proxy server 블록 모두에 `location ^~ /v1/images/`를 포함하므로 두 공개 진입점 모두 이미지 생성 배포 기준으로 사용할 수 있습니다. Web 진입점은 비스트리밍 `image_generation` 도구 호출을 위해 정확한 `/v1/responses` 경로에도 동일한 3600초 제한 시간을 적용합니다.

## 권장 예시 설정

다음 파일을 참고하세요.

- [`docker/nginx/nginx.conf`](../../../docker/nginx/nginx.conf)

이 예시는 다음을 함께 포함합니다.

- `manager.example.com -> codexmanager-service:48760`
- `web.example.com -> codexmanager-web:48761`
- HTTPS 리다이렉트
- 밑줄 헤더 허용
- 세션 관련 헤더 명시적 전달
- 스트리밍 친화적인 API 프록시 설정
- `/v1/images/generations` 및 `/v1/images/edits` 이미지 생성 프록시 설정

## 배포 후 검증 방법

최소 한 개의 핵심 경로는 반드시 검증하세요.

1. 같은 클라이언트, 같은 계정, 같은 대화를 사용해 배포 도메인으로 요청합니다.
2. 3~5회 연속 요청을 보냅니다.
3. 최신 `/v1/responses` 로그에서 `cached_input_tokens` 가 뚜렷하게 올라가는지 확인합니다.
4. 같은 계정, 같은 모델, 같은 프롬프트로 데스크톱 앱과 비교합니다.

수정이 제대로 먹혔다면:

- 첫 요청은 여전히 낮을 수 있고
- 두 번째 이후 요청에서 캐시 재사용이 더 잘 보이며
- 배포 결과가 데스크톱 동작에 훨씬 가까워져야 합니다

## 흔한 오진

### 오진 1: Docker 가 캐시를 망가뜨렸다

대부분 아닙니다. Docker 는 단지 런타임 컨테이너일 뿐이고, 실제 문제는 그 앞단 리버스 프록시에서 생기는 경우가 많습니다.

### 오진 2: Cloudflare 가 요청을 바꿨다

Cloudflare 가 경로를 복잡하게 만들 수는 있지만, 가장 흔한 직접 원인은 여전히 Nginx 가 밑줄 헤더를 기본적으로 버리는 것입니다.

### 오진 3: 캐시 수치가 낮으니 백엔드 로직이 틀렸다

반드시 그렇지는 않습니다. CodexManager 는 상류가 돌려준 usage 값을 그대로 기록합니다. 서비스에 도달하기 전 이미 스레드 앵커가 사라졌다면, 백엔드는 낮아진 캐시 적중 결과를 그대로 기록할 수밖에 없습니다.

## 점검 체크리스트

1. `http {}` 안에 `underscores_in_headers on;` 이 있는지 확인
2. `http {}` 안에 `ignore_invalid_headers off;` 이 있는지 확인
3. API 프록시 블록이 `conversation_id` 와 `session_id` 를 전달하는지 확인
4. 두 번째 프록시 계층이나 호스팅 패널이 헤더를 다시 쓰지 않는지 확인
5. 비교가 정말 같은 계정, 같은 모델, 같은 대화인지 확인
6. 첫 요청과 이후 반복 요청을 잘못 비교하고 있지 않은지 확인

## 소스 근거

- [`crates/service/src/gateway/request/incoming_headers.rs`](../../../crates/service/src/gateway/request/incoming_headers.rs)
- [`crates/service/src/gateway/request/session_affinity.rs`](../../../crates/service/src/gateway/request/session_affinity.rs)
- [`crates/service/src/gateway/request/official_responses_http.rs`](../../../crates/service/src/gateway/request/official_responses_http.rs)
- [`crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs`](../../../crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs)
- [`crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs`](../../../crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs)
