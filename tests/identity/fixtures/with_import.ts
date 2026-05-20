import { something } from "./other";

function greet(name: string): string {
  const message = "Hello, " + name;
  return message;
}

class AuthService {
  validateToken(token: string): boolean {
    const result = token.length > 0;
    return result;
  }
}
