import { createAccountWebCommands } from "./transport-web-commands/account";
import { createAggregateApiWebCommands } from "./transport-web-commands/aggregate-api";
import { createApiKeyWebCommands } from "./transport-web-commands/apikey";
import { createCodexProfileWebCommands } from "./transport-web-commands/codex-profile";
import { createGatewayWebCommands } from "./transport-web-commands/gateway";
import { createLoginWebCommands } from "./transport-web-commands/login";
import { createMiscWebCommands } from "./transport-web-commands/misc";
import { createQuotaWebCommands } from "./transport-web-commands/quota";
import type { WebCommandDescriptor, WebRpcCaller } from "./transport-web-commands/shared";

export type { InvokeParams, WebCommandDescriptor } from "./transport-web-commands/shared";

export function createWebCommandMap(postWebRpc: WebRpcCaller): Record<string, WebCommandDescriptor> {
  return {
    ...createMiscWebCommands(),
    ...createCodexProfileWebCommands(),
    ...createAccountWebCommands(postWebRpc),
    ...createQuotaWebCommands(),
    ...createAggregateApiWebCommands(),
    ...createLoginWebCommands(),
    ...createApiKeyWebCommands(),
    ...createGatewayWebCommands(),
  };
}
