# AgentSyncResponse

## Example Usage

```typescript
import { AgentSyncResponse } from "@alienplatform/manager-api/models";

let value: AgentSyncResponse = {};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `commandsUrl`                                                                                                                          | *string*                                                                                                                               | :heavy_minus_sign:                                                                                                                     | Public URL for the commands API. Cloud-deployed functions use this<br/>to poll for pending commands instead of the agent's local sync URL. |
| `target`                                                                                                                               | *any*                                                                                                                                  | :heavy_minus_sign:                                                                                                                     | N/A                                                                                                                                    |