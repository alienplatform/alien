# DeploymentDetailResponseAirgappedReport

When an air-gapped site's latest status was written and received.

## Example Usage

```typescript
import { DeploymentDetailResponseAirgappedReport } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseAirgappedReport = {
  receivedAt: new Date("2026-07-31T21:39:20.811Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `receivedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Manager clock time when the report was accepted.                                              |
| `writtenAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Site clock time; absent for reports from older clients.                                       |