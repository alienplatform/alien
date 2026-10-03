# SetDeploymentPinRequest

Body of `PUT /v1/deployments/{id}/pin`.

## Example Usage

```typescript
import { SetDeploymentPinRequest } from "@alienplatform/manager-api/models";

let value: SetDeploymentPinRequest = {};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `releaseId`                                                            | *string*                                                               | :heavy_minus_sign:                                                     | Release to pin to; absent unpins and returns to the channel's release. |