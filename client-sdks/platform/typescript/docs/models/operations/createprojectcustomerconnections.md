# CreateProjectCustomerConnections

Customer infrastructure offered by this Project through exact built-in or application-authored sources.

## Example Usage

```typescript
import { CreateProjectCustomerConnections } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectCustomerConnections = {
  schemaVersion: 1656.01,
  connections: {},
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `schemaVersion`                                                                            | *number*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `connections`                                                                              | [operations.CreateProjectConnections](../../models/operations/createprojectconnections.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |