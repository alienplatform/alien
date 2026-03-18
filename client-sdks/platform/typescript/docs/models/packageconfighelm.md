# PackageConfigHelm

## Example Usage

```typescript
import { PackageConfigHelm } from "@alienplatform/platform-api/models";

let value: PackageConfigHelm = {
  chartName: "<value>",
  description: "winged train redesign mob boiling saloon next",
  type: "helm",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `chartName`                             | *string*                                | :heavy_check_mark:                      | Chart name (e.g., "acme-operator")      |
| `description`                           | *string*                                | :heavy_check_mark:                      | Human-friendly description of the chart |
| `type`                                  | *"helm"*                                | :heavy_check_mark:                      | N/A                                     |