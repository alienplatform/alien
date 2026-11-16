# Values

## Overview

Deployment values held by this manager

### Available Operations

* [listValues](#listvalues)
* [putValues](#putvalues) - Stores the values, replacing any held in the same slots. With the header `If-None-Match: *` the request must carry exactly one value, and it is written only when its slot is empty, atomically: of several concurrent conditional writes to one slot exactly one succeeds. A manager that serves values (`capabilities.values` = 1) honours this header.
* [exportValues](#exportvalues) - Sends the stored values of the named slots to another manager, which writes them with the token given. The values go from this manager to the target; the response carries only the target's metadata. The caller's token must be an export token naming the target URL, and the request may name no other.
* [verifyValues](#verifyvalues)
* [deleteValue](#deletevalue)

## listValues

### Example Usage

<!-- UsageSnippet language="typescript" operationID="list_values" method="get" path="/v1/values/{scope}" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.values.listValues({
    scope: "<value>",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { valuesListValues } from "@alienplatform/manager-api/funcs/valuesListValues.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await valuesListValues(alienManager, {
    scope: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("valuesListValues failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.ListValuesRequest](../../models/operations/listvaluesrequest.md)                                                                                                   | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ValueSlotsResponse](../../models/valueslotsresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## putValues

Stores the values, replacing any held in the same slots. With the header `If-None-Match: *` the request must carry exactly one value, and it is written only when its slot is empty, atomically: of several concurrent conditional writes to one slot exactly one succeeds. A manager that serves values (`capabilities.values` = 1) honours this header.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="put_values" method="put" path="/v1/values/{scope}" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.values.putValues({
    scope: "<value>",
    putValuesRequest: {
      values: [
        {
          kind: "secret",
          slot: "<value>",
          value: "<value>",
        },
      ],
    },
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { valuesPutValues } from "@alienplatform/manager-api/funcs/valuesPutValues.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await valuesPutValues(alienManager, {
    scope: "<value>",
    putValuesRequest: {
      values: [
        {
          kind: "secret",
          slot: "<value>",
          value: "<value>",
        },
      ],
    },
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("valuesPutValues failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.PutValuesRequest](../../models/operations/putvaluesrequest.md)                                                                                                     | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ValueSlotsResponse](../../models/valueslotsresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## exportValues

Sends the stored values of the named slots to another manager, which writes them with the token given. The values go from this manager to the target; the response carries only the target's metadata. The caller's token must be an export token naming the target URL, and the request may name no other.

### Example Usage

<!-- UsageSnippet language="typescript" operationID="export_values" method="post" path="/v1/values/{scope}/export" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.values.exportValues("<value>", {
    slots: [],
    targetToken: "<value>",
    targetUrl: "https://secondary-presume.info/",
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { valuesExportValues } from "@alienplatform/manager-api/funcs/valuesExportValues.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await valuesExportValues(alienManager, "<value>", {
    slots: [],
    targetToken: "<value>",
    targetUrl: "https://secondary-presume.info/",
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("valuesExportValues failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `scope`                                                                                                                                                                        | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | dep:<deploymentId>                                                                                                                                                             |
| `exportValuesRequest`                                                                                                                                                          | [models.ExportValuesRequest](../../models/exportvaluesrequest.md)                                                                                                              | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.ValueSlotsResponse](../../models/valueslotsresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## verifyValues

### Example Usage

<!-- UsageSnippet language="typescript" operationID="verify_values" method="post" path="/v1/values/{scope}/verify" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const result = await alienManager.values.verifyValues("<value>", {
    values: [
      {
        slot: "<value>",
        value: "<value>",
      },
    ],
  });

  console.log(result);
}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { valuesVerifyValues } from "@alienplatform/manager-api/funcs/valuesVerifyValues.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await valuesVerifyValues(alienManager, "<value>", {
    values: [
      {
        slot: "<value>",
        value: "<value>",
      },
    ],
  });
  if (res.ok) {
    const { value: result } = res;
    console.log(result);
  } else {
    console.log("valuesVerifyValues failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `scope`                                                                                                                                                                        | *string*                                                                                                                                                                       | :heavy_check_mark:                                                                                                                                                             | dep:<deploymentId>                                                                                                                                                             |
| `verifyValuesRequest`                                                                                                                                                          | [models.VerifyValuesRequest](../../models/verifyvaluesrequest.md)                                                                                                              | :heavy_check_mark:                                                                                                                                                             | N/A                                                                                                                                                                            |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<[models.VerifyValuesResponse](../../models/verifyvaluesresponse.md)\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |

## deleteValue

### Example Usage

<!-- UsageSnippet language="typescript" operationID="delete_value" method="delete" path="/v1/values/{scope}/{slot}" -->
```typescript
import { AlienManager } from "@alienplatform/manager-api";

const alienManager = new AlienManager({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  await alienManager.values.deleteValue({
    scope: "<value>",
    slot: "<value>",
  });


}

run();
```

### Standalone function

The standalone function version of this method:

```typescript
import { AlienManagerCore } from "@alienplatform/manager-api/core.js";
import { valuesDeleteValue } from "@alienplatform/manager-api/funcs/valuesDeleteValue.js";

// Use `AlienManagerCore` for best tree-shaking performance.
// You can create one instance of it to use across an application.
const alienManager = new AlienManagerCore({
  serverURL: "https://api.example.com",
  bearer: process.env["ALIEN_MANAGER_BEARER"] ?? "",
});

async function run() {
  const res = await valuesDeleteValue(alienManager, {
    scope: "<value>",
    slot: "<value>",
  });
  if (res.ok) {
    const { value: result } = res;

  } else {
    console.log("valuesDeleteValue failed:", res.error);
  }
}

run();
```

### Parameters

| Parameter                                                                                                                                                                      | Type                                                                                                                                                                           | Required                                                                                                                                                                       | Description                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request`                                                                                                                                                                      | [operations.DeleteValueRequest](../../models/operations/deletevaluerequest.md)                                                                                                 | :heavy_check_mark:                                                                                                                                                             | The request object to use for the request.                                                                                                                                     |
| `options`                                                                                                                                                                      | RequestOptions                                                                                                                                                                 | :heavy_minus_sign:                                                                                                                                                             | Used to set various options for making HTTP requests.                                                                                                                          |
| `options.fetchOptions`                                                                                                                                                         | [RequestInit](https://developer.mozilla.org/en-US/docs/Web/API/Request/Request#options)                                                                                        | :heavy_minus_sign:                                                                                                                                                             | Options that are passed to the underlying HTTP request. This can be used to inject extra headers for examples. All `Request` options, except `method` and `body`, are allowed. |
| `options.retries`                                                                                                                                                              | [RetryConfig](../../lib/utils/retryconfig.md)                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                             | Enables retrying HTTP requests under certain failure conditions.                                                                                                               |

### Response

**Promise\<void\>**

### Errors

| Error Type                      | Status Code                     | Content Type                    |
| ------------------------------- | ------------------------------- | ------------------------------- |
| errors.AlienManagerDefaultError | 4XX, 5XX                        | \*/\*                           |
