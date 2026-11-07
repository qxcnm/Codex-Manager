import type {
  ModelDiscoveryList,
  ModelDiscoveryTask,
  ModelDiscoveryUpdateInput,
} from "@/types/model-discovery";
import { invoke, withAddr } from "./transport";

export const modelDiscoveryClient = {
  list(addr?: string | null): Promise<ModelDiscoveryList> {
    return invoke<ModelDiscoveryList>(
      "service_model_discovery_list",
      withAddr({ ...(addr === undefined ? {} : { addr: addr || null }) }),
    );
  },

  update(
    input: ModelDiscoveryUpdateInput,
    addr?: string | null,
  ): Promise<ModelDiscoveryTask> {
    return invoke<ModelDiscoveryTask>(
      "service_model_discovery_update",
      withAddr({
        ...input,
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
      { retries: 0 },
    );
  },
};
