# EnvironmentInfoAzure

Azure environment information

## Example Usage

```typescript
import { EnvironmentInfoAzure } from "@alienplatform/manager-api/models";

let value: EnvironmentInfoAzure = {
  location: "<value>",
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                 | Type                  | Required              | Description           |
| --------------------- | --------------------- | --------------------- | --------------------- |
| `location`            | *string*              | :heavy_check_mark:    | Azure location/region |
| `subscriptionId`      | *string*              | :heavy_check_mark:    | Azure subscription ID |
| `tenantId`            | *string*              | :heavy_check_mark:    | Azure tenant ID       |
| `platform`            | *"azure"*             | :heavy_check_mark:    | N/A                   |