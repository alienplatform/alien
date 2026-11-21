# CreateSetupEnrollmentRequestBody

## Example Usage

```typescript
import { CreateSetupEnrollmentRequestBody } from "@alienplatform/platform-api/models/operations";

let value: CreateSetupEnrollmentRequestBody = {
  location: {
    kind: "terraform",
    cloud: "gcp",
    resourceId: "<id>",
  },
};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `location`                                      | *operations.CreateSetupEnrollmentLocationUnion* | :heavy_check_mark:                              | N/A                                             |
| `expectedRevision`                              | *number*                                        | :heavy_minus_sign:                              | N/A                                             |