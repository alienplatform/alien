# MachinesLocalOverrideObservation

## Example Usage

```typescript
import { MachinesLocalOverrideObservation } from "@alienplatform/platform-api/models";

let value: MachinesLocalOverrideObservation = {
  baseAssignmentHash: "<value>",
  lifecycle: "<value>",
  replicaId: "<id>",
  workloadName: "<value>",
};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `activeDigest`       | *string*             | :heavy_minus_sign:   | N/A                  |
| `actor`              | *string*             | :heavy_minus_sign:   | N/A                  |
| `baseAssignmentHash` | *string*             | :heavy_check_mark:   | N/A                  |
| `baseDigest`         | *string*             | :heavy_minus_sign:   | N/A                  |
| `candidateDigest`    | *string*             | :heavy_minus_sign:   | N/A                  |
| `createdAt`          | *string*             | :heavy_minus_sign:   | N/A                  |
| `failureCategory`    | *string*             | :heavy_minus_sign:   | N/A                  |
| `fallbackDigest`     | *string*             | :heavy_minus_sign:   | N/A                  |
| `forcedStop`         | *boolean*            | :heavy_minus_sign:   | N/A                  |
| `healthySince`       | *string*             | :heavy_minus_sign:   | N/A                  |
| `incidentId`         | *string*             | :heavy_minus_sign:   | N/A                  |
| `lifecycle`          | *string*             | :heavy_check_mark:   | N/A                  |
| `phaseStartedAt`     | *string*             | :heavy_minus_sign:   | N/A                  |
| `replicaId`          | *string*             | :heavy_check_mark:   | N/A                  |
| `workloadName`       | *string*             | :heavy_check_mark:   | N/A                  |