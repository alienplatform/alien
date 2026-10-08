# CreateProjectLogCollectorRequest

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { CreateProjectLogCollectorRequest } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectLogCollectorRequest = {
  enabled: false,
  mode: "podApi",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `enabled`                                                                                  | *boolean*                                                                                  | :heavy_check_mark:                                                                         | Whether logs are collected by default; installers can override this value.                 |
| `mode`                                                                                     | [operations.CreateProjectModeRequest](../../models/operations/createprojectmoderequest.md) | :heavy_check_mark:                                                                         | Kubernetes log collection mechanism.                                                       |
