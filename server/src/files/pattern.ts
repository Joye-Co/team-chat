// TcSchema `pattern` without backtracking (SEC-4): the shared rule lives in packages/protocol, so
// the server and the bot SDK refuse and match the same patterns.
export { MAX_PATTERN_INPUT, patternProblem, patternTest, schemaPatternProblem, UnsupportedPattern } from "@chat/protocol";
