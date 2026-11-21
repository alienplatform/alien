# PersistImportedDeploymentRequestPreparedStackWhen

A boolean input condition controlling whether another input applies.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackWhen } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackWhen = {
  equals: false,
  input: "<value>",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `equals`                                                | *boolean*                                               | :heavy_check_mark:                                      | Boolean value for which this input applies.             |
| `input`                                                 | *string*                                                | :heavy_check_mark:                                      | ID of an unconditional boolean input in the same stack. |