# StackSettingsLogs

Application log handling for a deployment.

## Example Usage

```typescript
import { StackSettingsLogs } from "@alienplatform/platform-api/models";

let value: StackSettingsLogs = {};
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `parseApplicationLevels`                                                                                                                                     | *boolean*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                           | Normalize severity fields from supported structured application logs into<br/>the OTLP severity fields. The original log body is preserved. Disabled by<br/>default. |