# GetSetupEnrollmentResponse

Nonsecret setup progress.

## Example Usage

```typescript
import { GetSetupEnrollmentResponse } from "@alienplatform/platform-api/models/operations";

let value: GetSetupEnrollmentResponse = {
  reservation: {
    location: {
      kind: "cloudformation",
      stackName: "<value>",
      region: "<value>",
    },
    phase: "pending",
    canReplaceEnrollment: true,
    revision: 140792,
    expiresAt: new Date("2024-01-22T16:45:53.020Z"),
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `reservation`                                                    | [operations.Reservation](../../models/operations/reservation.md) | :heavy_check_mark:                                               | N/A                                                              |