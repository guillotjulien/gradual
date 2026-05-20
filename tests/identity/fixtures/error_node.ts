function greet(name: string): string {
  const message = "Hello, " + name;
  return message;
}

// Intentionally broken syntax to trigger an ERROR node in the tree
class Broken {
  method( {
    const x = 1;
  }
}
