# CreateProjectLogCollectorResponse

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { CreateProjectLogCollectorResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectLogCollectorResponse = {
  enabled: true,
  mode: "podApi",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `enabled`                                                                                    | *boolean*                                                                                    | :heavy_check_mark:                                                                           | Whether logs are collected by default; installers can override this value.                   |
| `mode`                                                                                       | [operations.CreateProjectModeResponse](../../models/operations/createprojectmoderesponse.md) | :heavy_check_mark:                                                                           | Kubernetes log collection mechanism.                                                         |