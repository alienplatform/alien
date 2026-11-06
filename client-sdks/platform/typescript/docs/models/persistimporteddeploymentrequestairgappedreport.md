# PersistImportedDeploymentRequestAirgappedReport

When an air-gapped site's latest status was written and received.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestAirgappedReport } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestAirgappedReport = {
  receivedAt: new Date("2026-07-27T23:59:27.391Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `receivedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | Manager clock time when the report was accepted.                                              |
| `writtenAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | Site clock time; absent for reports from older clients.                                       |