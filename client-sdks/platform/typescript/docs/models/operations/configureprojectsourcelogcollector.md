# ConfigureProjectSourceLogCollector

Default log collection mode in a generated Helm chart.

## Example Usage

```typescript
import { ConfigureProjectSourceLogCollector } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceLogCollector = {
  enabled: false,
  mode: "nodeAgent",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `enabled`                                                                                                      | *boolean*                                                                                                      | :heavy_check_mark:                                                                                             | Whether logs are collected by default; installers can override this value.                                     |
| `mode`                                                                                                         | [operations.ConfigureProjectSourceModeResponse](../../models/operations/configureprojectsourcemoderesponse.md) | :heavy_check_mark:                                                                                             | Kubernetes log collection mechanism.                                                                           |