# CurrentReleaseGenerate

Asks Alien to generate a secret input's value.

The value is an alphanumeric string (`A-Z`, `a-z`, `0-9`), so it is safe in
connection strings, command lines and environment variables. It is generated
once, when the deployment's input values are first resolved, and then kept
with the deployment's other input values.

## Example Usage

```typescript
import { CurrentReleaseGenerate } from "@alienplatform/platform-api/models";

let value: CurrentReleaseGenerate = {
  length: 332805,
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `length`                          | *number*                          | :heavy_check_mark:                | Number of characters to generate. |
