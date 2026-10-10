# Проблема низкого cache hit из-за Nginx reverse proxy

## Вывод

Если CodexManager развернут в Docker или как отдельный service и перед ним стоит Nginx с настройками по умолчанию, скрытая проблема обычно не в том, что "сломался кеш модели". Реальная причина в том, что **Nginx отбрасывает пользовательские заголовки с символом подчеркивания**.

CodexManager использует эти заголовки для стабильной привязки диалога к одному thread anchor:

- `conversation_id`
- `session_id`
- `x-client-request-id`
- `x-codex-turn-metadata`
- `x-codex-turn-state`

Если эти заголовки теряются на уровне reverse proxy, backend больше не может стабильно переиспользовать один и тот же thread anchor. Внешне это выглядит как заметно более низкий `cached_input_tokens` в веб-версии по сравнению с настольным приложением.

## Почему это происходит

По умолчанию Nginx плохо относится к заголовкам с подчеркиванием.

Если явно не включить:

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

такие заголовки, как `conversation_id` и `session_id`, могут считаться невалидными и быть отброшены до того, как попадут в CodexManager.

Это напрямую влияет на следующие участки:

1. `crates/service/src/gateway/request/incoming_headers.rs`
   извлекает `conversation_id`, `session_id`, `x-codex-turn-state` и связанные заголовки.
2. `crates/service/src/gateway/request/session_affinity.rs`
   вычисляет эффективный thread anchor.
3. `crates/service/src/gateway/request/official_responses_http.rs`
   записывает стабильный `prompt_cache_key` в тело запроса.

Если proxy срезает эти заголовки, backend вынужден деградировать до менее стабильной fallback-сессии, а повторное использование prompt cache становится хуже.

## Типичные симптомы

- В развернутом Web UI `cached_input_tokens` держится на уровне нескольких тысяч
- Локальное desktop-приложение на похожих запросах получает десятки тысяч
- Повторные запросы в рамках одного диалога не показывают заметного роста кеша
- Модель, аккаунт и промпт похожи, но онлайн-результат значительно хуже desktop

Важно:

- Низкий кеш на первом запросе сам по себе нормален
- Разные модели, аккаунты и диалоги не дают идентичные значения кеша
- Настоящий тревожный сигнал — устойчивый разрыв на нескольких запросах в одном и том же диалоге

## Правильное исправление

### 1. Разрешить заголовки с подчеркиванием

Добавьте в `http {}`:

```nginx
underscores_in_headers on;
ignore_invalid_headers off;
```

### 2. Явно прокинуть заголовки, связанные с сессией

Внутри `location` для API добавьте:

```nginx
proxy_set_header conversation_id $http_conversation_id;
proxy_set_header session_id $http_session_id;
proxy_set_header x-client-request-id $http_x_client_request_id;
proxy_set_header x-openai-subagent $http_x_openai_subagent;
proxy_set_header x-codex-beta-features $http_x_codex_beta_features;
proxy_set_header x-codex-turn-metadata $http_x_codex_turn_metadata;
proxy_set_header x-codex-turn-state $http_x_codex_turn_state;
```

Это лучше считать базовой частью деплоя, а не необязательной оптимизацией.

### 3. Отключить буферизацию для streaming API

Рекомендуемые настройки:

```nginx
proxy_buffering off;
proxy_request_buffering off;
proxy_read_timeout 3600s;
proxy_send_timeout 3600s;
```

Это не создает cache hit само по себе, но уменьшает вмешательство proxy для Responses, SSE и WebSocket-трафика.

### 4. Выделить `/v1/images/` в отдельный консервативный proxy-блок

CodexManager поддерживает совместимые endpoints `/v1/images/generations` и `/v1/images/edits`. Для этого пути риски proxy отличаются от обычных текстовых запросов:

- `/v1/images/edits` может загружать multipart-изображения с большим request body
- генерация изображений может занимать больше времени, чем первый токен текстового ответа
- ответы `b64_json` могут быть большими, а стандартная буферизация proxy увеличивает риск задержек или обрыва

Для `/v1/images/` рекомендуется как минимум:

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

Текущий `docker/nginx/nginx.conf` уже содержит блок `location ^~ /v1/images/` как для прямого service, так и для Web proxy, поэтому оба публичных входа могут использоваться как базовая конфигурация image generation. Для точного маршрута `/v1/responses` на Web-входе также задан тайм-аут 3600 секунд, чтобы поддержать нестриминговые вызовы инструмента `image_generation`.

## Рекомендуемый пример конфигурации

Смотрите:

- [`docker/nginx/nginx.conf`](../../../docker/nginx/nginx.conf)

Пример покрывает:

- `manager.example.com -> codexmanager-service:48760`
- `web.example.com -> codexmanager-web:48761`
- HTTPS redirect
- поддержку заголовков с подчеркиванием
- явную передачу session-заголовков
- настройки API proxy для streaming-сценариев
- настройки proxy для `/v1/images/generations` и `/v1/images/edits`

## Как проверять после деплоя

Проверьте хотя бы один критический путь:

1. Используйте тот же клиент, тот же аккаунт и тот же диалог на развернутом домене.
2. Отправьте 3–5 последовательных запросов.
3. Убедитесь, что в новых логах `/v1/responses` явно растет `cached_input_tokens`.
4. Сравните результат с desktop-приложением при том же аккаунте, модели и промпте.

Если исправление сработало:

- первый запрос может оставаться низким
- на втором и последующих запросах кеш должен расти заметнее
- поведение развернутой версии должно приблизиться к desktop

## Частые ошибочные выводы

### Ошибочный вывод 1: Docker сломал кеш

Обычно нет. Docker — это только среда выполнения. Регресс чаще всего вносит reverse proxy перед ним.

### Ошибочный вывод 2: Cloudflare изменил запрос

Cloudflare может усложнять цепочку запроса, но самый частый прямой источник проблемы — стандартное поведение Nginx, который отбрасывает заголовки с подчеркиванием.

### Ошибочный вывод 3: Низкий кеш значит, что backend написан неправильно

Не обязательно. CodexManager логирует usage так, как его возвращает upstream. Если thread anchor потерян еще до попадания запроса в service, backend может только зафиксировать деградировавший результат.

## Чеклист диагностики

1. Проверьте наличие `underscores_in_headers on;` в `http {}`
2. Проверьте наличие `ignore_invalid_headers off;` в `http {}`
3. Проверьте, что API proxy блок передает `conversation_id` и `session_id`
4. Убедитесь, что нет второго proxy-слоя или панели, переписывающей заголовки
5. Проверьте, что сравнение действительно идет для одного аккаунта, одной модели и одного диалога
6. Убедитесь, что вы не сравниваете первый запрос с повторными запросами

## Ссылки на код

- [`crates/service/src/gateway/request/incoming_headers.rs`](../../../crates/service/src/gateway/request/incoming_headers.rs)
- [`crates/service/src/gateway/request/session_affinity.rs`](../../../crates/service/src/gateway/request/session_affinity.rs)
- [`crates/service/src/gateway/request/official_responses_http.rs`](../../../crates/service/src/gateway/request/official_responses_http.rs)
- [`crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs`](../../../crates/service/src/gateway/observability/http_bridge/aggregate/output_text.rs)
- [`crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs`](../../../crates/service/src/gateway/observability/http_bridge/stream_readers/responses_from_anthropic.rs)
