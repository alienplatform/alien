# Reservation

## Example Usage

```typescript
import { Reservation } from "@alienplatform/platform-api/models/operations";

let value: Reservation = {
  location: {
    kind: "terraform",
    cloud: "azure",
    resourceId: "<id>",
  },
  phase: "pending",
  canReplaceEnrollment: true,
  revision: 305369,
  expiresAt: new Date("2024-03-24T12:07:35.024Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `location`                                                                                    | *operations.GetSetupEnrollmentLocationUnion*                                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `phase`                                                                                       | [operations.Phase](../../models/operations/phase.md)                                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `canReplaceEnrollment`                                                                        | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `revision`                                                                                    | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `expiresAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |