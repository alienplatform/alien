# NewManagerRequest

## Example Usage

```typescript
import { NewManagerRequest } from "@alienplatform/platform-api/models";

let value: NewManagerRequest = {
  name: "<value>",
  platform: "local",
  targets: [],
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                                   | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `platform`                                                                                                               | [models.NewManagerRequestPlatform](../models/newmanagerrequestplatform.md)                                               | :heavy_check_mark:                                                                                                       | Platform where the Manager will be deployed (must be aws, gcp, or azure)                                                 |
| `targets`                                                                                                                | [models.NewManagerRequestTarget](../models/newmanagerrequesttarget.md)[]                                                 | :heavy_check_mark:                                                                                                       | Platforms this Manager can manage (can include local, kubernetes, etc.)                                                  |
| `otlpConfig`                                                                                                             | [models.OtlpConfig](../models/otlpconfig.md)                                                                             | :heavy_minus_sign:                                                                                                       | Optional external OTLP config for forwarding logs to Axiom, Datadog, etc. Falls back to built-in DeepStore when not set. |