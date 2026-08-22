# CreateManagerResponseLogs1

Application log handling for a deployment.

## Example Usage

```typescript
import { CreateManagerResponseLogs1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseLogs1 = {};
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `parseApplicationLevels`                                                                                                                                     | *boolean*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                           | Normalize severity fields from supported structured application logs into<br/>the OTLP severity fields. The original log body is preserved. Disabled by<br/>default. |