import { createAccountWebCommands } from "./transport-web-commands/account";
import { createAggregateApiWebCommands } from "./transport-web-commands/aggregate-api";
import { createApiKeyWebCommands } from "./transport-web-commands/apikey";
import { createCodexProfileWebCommands } from "./transport-web-commands/codex-profile";
import { createCodexSkillsWebCommands } from "./transport-web-commands/codex-skills";
import { createGatewayWebCommands } from "./transport-web-commands/gateway";
import { createLoginWebCommands } from "./transport-web-commands/login";
import { createMiscWebCommands } from "./transport-web-commands/misc";
import { createModelDiscoveryWebCommands } from "./transport-web-commands/model-discovery";
import { createProxyProfilesWebCommands } from "./transport-web-commands/proxy-profiles";
import { createQuotaWebCommands } from "./transport-web-commands/quota";
import { createServiceHealthWebCommands } from "./transport-web-commands/service-health";
import type { WebCommandDescriptor, WebRpcCaller } from "./transport-web-commands/shared";

export type { InvokeParams, WebCommandDescriptor } from "./transport-web-commands/shared";

export function createWebCommandMap(postWebRpc: WebRpcCaller): Record<string, WebCommandDescriptor> {
  return {
    ...createMiscWebCommands(),
    ...createModelDiscoveryWebCommands(),
    ...createCodexProfileWebCommands(),
    ...createCodexSkillsWebCommands(),
    ...createAccountWebCommands(postWebRpc),
    ...createProxyProfilesWebCommands(),
    ...createQuotaWebCommands(),
    ...createServiceHealthWebCommands(),
    ...createAggregateApiWebCommands(),
    ...createLoginWebCommands(),
    ...createApiKeyWebCommands(),
    ...createGatewayWebCommands(),
  };
}
