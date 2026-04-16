# CreateCommandRequest

## Example Usage

```typescript
import { CreateCommandRequest } from "@alienplatform/platform-api/models";

let value: CreateCommandRequest = {
  deploymentId: "ag_pnj2da55wi5sxbdcav9t273je",
  name: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `deploymentId`                                                                                | *string*                                                                                      | :heavy_check_mark:                                                                            | Target deployment ID                                                                          | ag_pnj2da55wi5sxbdcav9t273je                                                                  |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Command name (e.g., 'analyze-repository')                                                     |                                                                                               |
| `initialState`                                                                                | [models.InitialState](../models/initialstate.md)                                              | :heavy_minus_sign:                                                                            | Initial state (PENDING_UPLOAD if params require upload, PENDING if inline)                    |                                                                                               |
| `deadline`                                                                                    | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Optional deadline for command execution                                                       |                                                                                               |
| `requestSizeBytes`                                                                            | *number*                                                                                      | :heavy_minus_sign:                                                                            | Size of command params in bytes                                                               |                                                                                               |