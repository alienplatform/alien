# DeploymentAirgappedReport

When an air-gapped site's latest status was written and received.

## Example Usage

```typescript
import { DeploymentAirgappedReport } from "@alienplatform/platform-api/models";

let value: DeploymentAirgappedReport = {
  receivedAt: new Date("2024-04-30T08:42:05.886Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `receivedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Manager clock time when the report was accepted.                                              |
| `writtenAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Site clock time; absent for reports from older clients.                                       |