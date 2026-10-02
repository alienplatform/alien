# GetDeploymentLogsRequest

## Example Usage

```typescript
import { GetDeploymentLogsRequest } from "@alienplatform/manager-api/models/operations";

let value: GetDeploymentLogsRequest = {
  id: "<id>",
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                     | *string*                                                                                                                                 | :heavy_check_mark:                                                                                                                       | Deployment ID                                                                                                                            |
| `limit`                                                                                                                                  | *number*                                                                                                                                 | :heavy_minus_sign:                                                                                                                       | Most recent entries to return (default 200, max 5000).                                                                                   |
| `since`                                                                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                            | :heavy_minus_sign:                                                                                                                       | Only entries at or after this time (RFC 3339). Inclusive, so a caller<br/>following logs doesn't miss entries that share its last timestamp. |