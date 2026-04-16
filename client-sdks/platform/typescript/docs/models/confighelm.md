# ConfigHelm

Configuration for the Helm chart package

## Example Usage

```typescript
import { ConfigHelm } from "@alienplatform/platform-api/models";

let value: ConfigHelm = {
  chartName: "<value>",
  description: "idle ew yippee approach abaft",
  type: "helm",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `chartName`                             | *string*                                | :heavy_check_mark:                      | Chart name (e.g., "acme-operator")      |
| `description`                           | *string*                                | :heavy_check_mark:                      | Human-friendly description of the chart |
| `type`                                  | *"helm"*                                | :heavy_check_mark:                      | N/A                                     |