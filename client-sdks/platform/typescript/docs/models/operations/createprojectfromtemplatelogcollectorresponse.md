# CreateProjectFromTemplateLogCollectorResponse

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { CreateProjectFromTemplateLogCollectorResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateLogCollectorResponse = {
  enabled: true,
  mode: "podApi",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                            | *boolean*                                                                                                            | :heavy_check_mark:                                                                                                   | Whether logs are collected by default; installers can override this value.                                           |
| `mode`                                                                                                               | [operations.CreateProjectFromTemplateModeResponse](../../models/operations/createprojectfromtemplatemoderesponse.md) | :heavy_check_mark:                                                                                                   | Kubernetes log collection mechanism.                                                                                 |