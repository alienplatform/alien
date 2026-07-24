# AgentSessionToolResultEventPayload

## Example Usage

```typescript
import { AgentSessionToolResultEventPayload } from "@alienplatform/platform-api/models";

let value: AgentSessionToolResultEventPayload = {
  toolCallId: "<id>",
  toolName: "<value>",
  ok: true,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `toolCallId`       | *string*           | :heavy_check_mark: | N/A                |
| `toolName`         | *string*           | :heavy_check_mark: | N/A                |
| `ok`               | *boolean*          | :heavy_check_mark: | N/A                |
| `output`           | *any*              | :heavy_minus_sign: | N/A                |
