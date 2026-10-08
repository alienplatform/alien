# UnregisteredSetup

## Example Usage

```typescript
import { UnregisteredSetup } from "@alienplatform/platform-api/models/operations";

let value: UnregisteredSetup = {
  deploymentGroupId: "<id>",
  name: "<value>",
  platform: "ecs",
  createdAt: new Date("2024-10-20T13:39:49.932Z"),
  valuesExpireAt: new Date("2025-08-14T09:41:45.137Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentGroupId`                                                                           | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `platform`                                                                                    | [operations.UnregisteredSetupPlatform](../../models/operations/unregisteredsetupplatform.md)  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `valuesExpireAt`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | When the newest installation values stop working. Resuming setup can replace them.            |
