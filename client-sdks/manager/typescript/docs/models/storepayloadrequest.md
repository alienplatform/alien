# StorePayloadRequest

Request to store payload data directly in KV by command_id.

This bypasses the normal command lifecycle (create → dispatch → respond)
and writes params/response directly into KV. Used by the demo service
to populate payload data for commands created outside the command flow.

## Example Usage

```typescript
import { StorePayloadRequest } from "@alienplatform/manager-api/models";

let value: StorePayloadRequest = {};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `params`                 | *models.BodySpec*        | :heavy_minus_sign:       | N/A                      |
| `response`               | *models.CommandResponse* | :heavy_minus_sign:       | N/A                      |