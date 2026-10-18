import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

/**
 * Unmount what a test rendered.
 *
 * Without this, a test that renders a panel leaves it in the document and the
 * next test's queries find both. The failures that causes look like wrong
 * component logic, not like a missing cleanup, which is why it is explicit.
 */
afterEach(cleanup);
