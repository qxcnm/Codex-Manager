# Experimental BPS forwarding

CodexManager includes its own Responses-to-ChatGPT Excel/Basispoints adapter. No external bridge executable is required. This changes an account's generation route; it does not guarantee improved model quality or grant additional permissions or quota.

## Setup

An administrator can use **BPS forwarding** on an account card or list row. It is disabled by default. Enabling it first opens a risk confirmation showing the number of affected accounts. The notice explains that this experimental feature does not guarantee improved model quality or prevent reduced capabilities, and that requests may fail, features may be incompatible, and account use may carry risks. Users must assess and accept those risks themselves. **Cancel** leaves the saved state unchanged and starts neither authorization nor a settings update.

For an account with valid BPS credentials, choose **Accept risks and enable**. Otherwise, choose **Accept risks and authorize**, then **Start BPS authorization** in the authorization dialog. Open the displayed browser page and enter the device code. Sign in with the selected account: both account and available user identity are checked. Starting authorization from the switch enables forwarding after authorization completes; the separate key-icon authorization action only saves credentials.

**Accounts already imported into or signed in to Codex still need independent BPS authorization on first use.** Existing Codex credentials serve the original Codex route and are not used as BPS authorization by this feature. Use the same ChatGPT account; a new account is not required. The two sets of credentials are stored and maintained separately.

**Enable device code login before starting authorization:** in ChatGPT, open **Settings → Security** and enable device code login, then return to CodexManager to start BPS authorization. Workspace accounts may need an administrator to enable it. CodexManager does not change this setting automatically, and enabling it does not guarantee BPS access.

For batch changes, select accounts and use **Account actions**. Every selected account must have valid BPS authorization before a batch can be enabled; otherwise, the entire selection remains unchanged. An eligible batch receives one risk confirmation for the exact selection, and no update is submitted until it is accepted. Canceling changes no accounts. Disabling BPS does not require risk confirmation and restores the original Codex upstream for subsequent requests selecting that account.

Desktop and service-mode Web UI use the same settings and backend. Requests must go through the CodexManager gateway; direct account connections bypass this feature. Independent BPS credentials are stored in `account_bps_settings`, are never returned by account-list RPC, survive ordinary account imports, and are removed with the account. Codex credentials and refresh are unaffected. BPS credentials currently require renewed device authorization after expiry; automatic BPS refresh is not enabled. An authorized label indicates current credentials, not verified generation access.

## Behavior and limits

- Generation uses the selected account's independent BPS credentials, account proxy, and gateway timeouts. The adapter supports Responses, converted Chat Completions, and client WebSocket requests, retaining the existing streaming/non-streaming delivery formats.
- While any account in the pool has BPS enabled, new client WebSocket connections use the existing HTTP bridge before account routing selects the upstream. Requests selecting an account with BPS disabled still use the original Codex upstream; enabling one account does not send every account to BPS. Existing native WebSocket connections may require reconnection after a route change.
- The requested model is retained. Reasoning levels are normalized to `low`, `medium`, `high`, or `xhigh`; `max/ultra` map to `xhigh`, `none/minimal` map to `low`. Upstream response model, reasoning, and usage fields are preserved instead of being replaced with the requested values.
- Instructions and full history are converted. Function/custom/namespaced client tools use a BPS-native carrier, then are restored for client execution. Only completed, validated calls are emitted; tools run sequentially. HTTP `previous_response_id` and `item_reference` require expanded history. The existing client-WebSocket bridge handles history expansion.
- HTTPS image URLs and supported encrypted history are retained. Default context compaction threshold is `200000`, overridable by a valid `context_management` compaction setting.
- Hosted search, file search, code execution, and image generation are unavailable. Automatic tool declarations for these capabilities are omitted and disclosed in the adapter prompt and `metadata.codexmanager_bps_omitted_tools`; explicitly requiring these capabilities is rejected.
- Inline images, files, audio, background generation, constrained JSON output, and unsupported parameters such as `temperature`, `top_p`, `max_output_tokens`, `max_tool_calls`, and `truncation` are rejected. Disable BPS to use the original route for these requests.
- Only `auto/default` service tiers are supported. Fast, Ultrafast, and Flex selections are rejected. Existing account-pool eligibility, group/key ownership, and model routing boundaries still apply. Codex quota displays do not measure BPS quota.
- BPS errors do not invalidate or refresh the original Codex credentials. Enabled requests do not silently fall back to Codex, and dispatched requests are not automatically replayed.

## Authorization troubleshooting

If the browser authorization page reports `deviceauth_disabled`, check whether device code login is disabled. For a personal account, check **Settings → Security** in ChatGPT. Workspace accounts may require an administrator to enable device code login in workspace permissions. CodexManager does not change these settings automatically; the decision belongs to you or your workspace administrator.

After confirming that device code login is allowed, return to the BPS dialog and choose **Open BPS authorization** again. If the code has expired, start authorization again. Allowing device code login addresses this login-setting restriction only; it does not guarantee BPS access or access to the requested model. See [OpenAI authentication guidance](https://learn.chatgpt.com/zh-Hans/docs/auth).

## Verification

Choose **Test account** from the card or list row's account menu. The test follows that account's BPS switch: enabled tests BPS, disabled tests the original Codex route. BPS testing supports text models only. Image-generation tests return an explicit error instead of using the original route. A BPS test failure does not invalidate the original Codex login or alter its account-health state. Check the final completion event and actual text output; connection success alone does not establish successful generation.

The internal upstream protocol and account/model access can change. Mock OAuth/WebSocket tests verify local behavior; real generation still requires the user's independent authorization and a separate live check. A successful handshake, HTTP 200, or requested `xhigh` setting does not establish output quality.
