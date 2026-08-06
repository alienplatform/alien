exports.handler = async _event => {
  return {
    statusCode: 200,
    body: JSON.stringify({ message: "Hello from test Lambda!" }),
  }
}
