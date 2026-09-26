# IncidentListResponse

## Example Usage

```typescript
import { IncidentListResponse } from "@alienplatform/platform-api/models";

let value: IncidentListResponse = {
  items: [],
  nextCursor: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `items`                                    | [models.Incident](../models/incident.md)[] | :heavy_check_mark:                         | N/A                                        |
| `nextCursor`                               | *string*                                   | :heavy_check_mark:                         | N/A                                        |