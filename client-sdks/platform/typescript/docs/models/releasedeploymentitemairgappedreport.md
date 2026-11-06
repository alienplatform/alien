# ReleaseDeploymentItemAirgappedReport

When an air-gapped site's latest status was written and received.

## Example Usage

```typescript
import { ReleaseDeploymentItemAirgappedReport } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemAirgappedReport = {
  receivedAt: new Date("2024-02-16T01:55:47.122Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `receivedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Manager clock time when the report was accepted.                                              |
| `writtenAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Site clock time; absent for reports from older clients.                                       |