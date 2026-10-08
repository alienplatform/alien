# InstallationsData

## Example Usage

```typescript
import { InstallationsData } from "@alienplatform/platform-api/models/operations";

let value: InstallationsData = {
  items: [],
  unregisteredSetups: [
    {
      deploymentGroupId: "<id>",
      name: "<value>",
      platform: "kubernetes",
      createdAt: new Date("2024-03-02T14:19:50.314Z"),
      valuesExpireAt: new Date("2024-04-11T22:30:19.364Z"),
    },
  ],
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `items`                                                                                                            | [operations.GetRemoteOperatorProjectSummaryItem](../../models/operations/getremoteoperatorprojectsummaryitem.md)[] | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `unregisteredSetups`                                                                                               | [operations.UnregisteredSetup](../../models/operations/unregisteredsetup.md)[]                                     | :heavy_check_mark:                                                                                                 | Setups whose installation values were created but whose Operator never registered, newest first.                   |