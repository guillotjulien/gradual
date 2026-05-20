class AuthService {
  validateToken(token: string): boolean {
    const result = token.length > 0;
    return result;
  }
}

function greet(name: string): string {
  const message = "Hello, " + name;
  return message;
}
