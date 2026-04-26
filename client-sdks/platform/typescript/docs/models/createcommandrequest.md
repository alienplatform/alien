# CreateCommandRequest

## Example Usage

```typescript
import { CreateCommandRequest } from "@alienplatform/platform-api/models";

let value: CreateCommandRequest = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Target deployment ID                                                                          | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                  |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Command name (e.g., 'analyze-repository')                                                     |                                                                                               |
| `initialState`                                                                                | [models.InitialState](../models/initialstate.md)                                              | :heavy_minus_sign:                                                                            | Initial state (PENDING_UPLOAD if params require upload, PENDING if inline)                    |                                                                                               |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional deadline for command execution                                                       |                                                                                               |
| `requestSizeBytes`                                                                            | *number*                                                                                      | :heavy_minus_sign:                                                                            | Size of command params in bytes                                                               |                                                                                               |