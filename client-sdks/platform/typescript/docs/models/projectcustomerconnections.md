# ProjectCustomerConnections

Customer infrastructure offered by this Project through exact built-in or application-authored sources.

## Example Usage

```typescript
import { ProjectCustomerConnections } from "@alienplatform/platform-api/models";

let value: ProjectCustomerConnections = {
  schemaVersion: 628.23,
  connections: {},
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `schemaVersion`                                              | *number*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `connections`                                                | [models.ProjectConnections](../models/projectconnections.md) | :heavy_check_mark:                                           | N/A                                                          |