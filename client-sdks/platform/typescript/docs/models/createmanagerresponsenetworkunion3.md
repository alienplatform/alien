# CreateManagerResponseNetworkUnion3


## Supported Types

### `models.CreateManagerResponseNetworkUseDefault3`

```typescript
const value: models.CreateManagerResponseNetworkUseDefault3 = {
  type: "use-default",
};
```

### `models.CreateManagerResponseNetworkCreate3`

```typescript
const value: models.CreateManagerResponseNetworkCreate3 = {
  type: "create",
};
```

### `models.CreateManagerResponseNetworkByoVpcAws3`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcAws3 = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.CreateManagerResponseNetworkByoVpcGcp3`

```typescript
const value: models.CreateManagerResponseNetworkByoVpcGcp3 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.CreateManagerResponseNetworkByoVnetAzure3`

```typescript
const value: models.CreateManagerResponseNetworkByoVnetAzure3 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

