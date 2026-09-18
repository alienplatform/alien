# RevokeContainerRegistryCredentialRequest

## Example Usage

```typescript
import { RevokeContainerRegistryCredentialRequest } from "@alienplatform/platform-api/models/operations";

let value: RevokeContainerRegistryCredentialRequest = {
  id: "dg_r27ict8c7vcgsumpj90ackf7b",
  credentialId: "crcred_oz1xjr82f37j17g4gtmyu",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              | Example                                                  |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `id`                                                     | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the deployment group.              | dg_r27ict8c7vcgsumpj90ackf7b                             |
| `credentialId`                                           | *string*                                                 | :heavy_check_mark:                                       | Unique identifier for the container registry credential. | crcred_oz1xjr82f37j17g4gtmyu                             |