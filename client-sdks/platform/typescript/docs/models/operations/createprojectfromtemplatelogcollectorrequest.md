# CreateProjectFromTemplateLogCollectorRequest

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { CreateProjectFromTemplateLogCollectorRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateLogCollectorRequest = {
  enabled: true,
  mode: "podApi",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                                          | *boolean*                                                                                                          | :heavy_check_mark:                                                                                                 | Whether logs are collected by default; installers can override this value.                                         |
| `mode`                                                                                                             | [operations.CreateProjectFromTemplateModeRequest](../../models/operations/createprojectfromtemplatemoderequest.md) | :heavy_check_mark:                                                                                                 | Kubernetes log collection mechanism.                                                                               |
