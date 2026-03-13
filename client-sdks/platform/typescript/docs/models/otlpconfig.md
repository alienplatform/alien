# OtlpConfig

Optional external OTLP config for forwarding logs to Axiom, Datadog, etc. Falls back to built-in DeepStore when not set.

## Example Usage

```typescript
import { OtlpConfig } from "@aliendotdev/platform-api/models";

let value: OtlpConfig = {
  logsEndpoint: "https://lazy-fishery.name",
  logsAuthHeader: "<value>",
};
```

## Fields

| Field                                                                                                 | Type                                                                                                  | Required                                                                                              | Description                                                                                           |
| ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `logsEndpoint`                                                                                        | *string*                                                                                              | :heavy_check_mark:                                                                                    | External OTLP logs endpoint (e.g. https://api.axiom.co/v1/logs)                                       |
| `logsAuthHeader`                                                                                      | *string*                                                                                              | :heavy_check_mark:                                                                                    | Auth header in 'key=value,...' format (e.g. 'authorization=Bearer <token>,x-axiom-dataset=<dataset>') |