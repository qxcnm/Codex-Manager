import type {
  ServiceHealthActionResult,
  ServiceHealthReportInput,
  ServiceHealthReportResult,
  ServiceHealthSummary,
} from "@/types/service-health";
import { invoke, withAddr } from "./transport";

export const serviceHealthClient = {
  summary(
    addr?: string | null,
  ): Promise<ServiceHealthSummary> {
    return invoke<ServiceHealthSummary>(
      "service_health_summary",
      withAddr({
        ...(addr === undefined ? {} : { addr: addr || null }),
      }),
    );
  },

  report(
    input: ServiceHealthReportInput,
    addr?: string | null,
  ): Promise<ServiceHealthReportResult> {
    return invoke<ServiceHealthReportResult>(
      "service_health_report",
      withAddr({ ...input, ...(addr === undefined ? {} : { addr: addr || null }) }),
      { retries: 0 },
    );
  },

  resolve(id: string, addr?: string | null): Promise<ServiceHealthActionResult> {
    return invoke<ServiceHealthActionResult>(
      "service_health_resolve",
      withAddr({ id, ...(addr === undefined ? {} : { addr: addr || null }) }),
      { retries: 0 },
    );
  },

};
