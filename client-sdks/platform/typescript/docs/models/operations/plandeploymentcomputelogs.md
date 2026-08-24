# PlanDeploymentComputeLogs

Application log handling for a deployment.

## Example Usage

```typescript
import { PlanDeploymentComputeLogs } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeLogs = {};
```

## Fields

| Field                                                                                                                                                        | Type                                                                                                                                                         | Required                                                                                                                                                     | Description                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `parseApplicationLevels`                                                                                                                                     | *boolean*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                           | Normalize severity fields from supported structured application logs into<br/>the OTLP severity fields. The original log body is preserved. Disabled by<br/>default. |