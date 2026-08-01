# SyncListResponseProfileGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponseProfileGcpResource } from "@alienplatform/platform-api/models";

let value: SyncListResponseProfileGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `condition`                                            | *models.SyncListResponseProfileResourceConditionUnion* | :heavy_minus_sign:                                     | N/A                                                    |
| `scope`                                                | *string*                                               | :heavy_check_mark:                                     | Scope (project/resource level)                         |