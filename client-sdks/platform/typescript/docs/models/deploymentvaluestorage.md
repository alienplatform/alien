# DeploymentValueStorage

Where values entered for this group's setup links are stored. The platform for now: the group's value-access endpoint answers 409 until that changes.

## Example Usage

```typescript
import { DeploymentValueStorage } from "@alienplatform/platform-api/models";

let value: DeploymentValueStorage = {
  storedIn: "manager",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `storedIn`                               | [models.StoredIn](../models/storedin.md) | :heavy_check_mark:                       | N/A                                      |
| `isAlienCloud`                           | *boolean*                                | :heavy_minus_sign:                       | N/A                                      |
| `managerId`                              | *string*                                 | :heavy_minus_sign:                       | N/A                                      |
| `managerName`                            | *string*                                 | :heavy_minus_sign:                       | N/A                                      |