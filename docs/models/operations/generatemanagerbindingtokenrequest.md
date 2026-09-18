# GenerateManagerBindingTokenRequest

## Example Usage

```typescript
import { GenerateManagerBindingTokenRequest } from "@alienplatform/platform-api/models/operations";

let value: GenerateManagerBindingTokenRequest = {
  id: "mgr_enxscjrqiiu2lrc672hwwuc5",
  generateManagerBindingTokenRequest: {
    deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  },
};
```

## Fields

| Field                                                                                           | Type                                                                                            | Required                                                                                        | Description                                                                                     | Example                                                                                         |
| ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `id`                                                                                            | *string*                                                                                        | :heavy_check_mark:                                                                              | Unique identifier for a manager.                                                                | mgr_enxscjrqiiu2lrc672hwwuc5                                                                    |
| `generateManagerBindingTokenRequest`                                                            | [models.GenerateManagerBindingTokenRequest](../../models/generatemanagerbindingtokenrequest.md) | :heavy_check_mark:                                                                              | N/A                                                                                             |                                                                                                 |