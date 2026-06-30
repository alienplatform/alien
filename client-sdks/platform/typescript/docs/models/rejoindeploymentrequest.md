# RejoinDeploymentRequest

Request schema for re-acquiring a deployment-scoped sync token after local state loss.

## Example Usage

```typescript
import { RejoinDeploymentRequest } from "@alienplatform/platform-api/models";

let value: RejoinDeploymentRequest = {
  name: "<value>",
};
```

## Fields

| Field                                                                           | Type                                                                            | Required                                                                        | Description                                                                     |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `name`                                                                          | *string*                                                                        | :heavy_check_mark:                                                              | Deployment name to rejoin. Must already exist in the caller's deployment group. |