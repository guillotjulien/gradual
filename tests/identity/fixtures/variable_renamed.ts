function greet(name: string): string {
  const greeting = "Hello, " + name;
  return greeting;
}

class AuthService {
  validateToken(token: string): boolean {
    const result = token.length > 0;
    return result;
  }
}
