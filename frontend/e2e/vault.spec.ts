import { test, expect, Page } from "@playwright/test";
import { defaultState, installMocks } from "./rpcMock";
import { USER_PUBKEY, OTHER_PUBKEY } from "./fixtures";

const SHORT_ADDR = `${USER_PUBKEY.slice(0, 4)}…${USER_PUBKEY.slice(-4)}`;

async function setup(page: Page, admin: string) {
  await installMocks(page, defaultState(USER_PUBKEY, admin));
  await page.goto("/");
}

test.describe("Aqua vault UI (connected as admin)", () => {
  test.beforeEach(async ({ page }) => {
    await setup(page, USER_PUBKEY);
  });

  test("connects and disconnects a wallet", async ({ page }) => {
    const connect = page.getByRole("button", { name: "Connect Wallet" }).first();
    await expect(connect).toBeVisible();
    await connect.click();
    await expect(page.getByRole("button", { name: /Disconnect/ })).toBeVisible();
    await page.getByRole("button", { name: /Disconnect/ }).click();
    await expect(
      page.getByRole("button", { name: "Connect Wallet" }).first(),
    ).toBeVisible();
  });

  test("renders vault stats from the mocked RPC", async ({ page }) => {
    await expect(page.getByText("Current Prize Pool")).toBeVisible();
    await expect(page.getByText("$0.25")).toBeVisible();
    await expect(page.getByText("Total Value Locked")).toBeVisible();
    await expect(page.getByText("$10")).toBeVisible();
    await expect(page.getByText("1 saver")).toBeVisible();
  });

  test("deposit updates the DepositCard from the mocked response", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Connect Wallet" }).first().click();
    await page.getByPlaceholder("0.00").fill("2.55");
    await page.getByRole("button", { name: "Deposit USDC" }).click();

    await expect(page.getByText(/Deposited \$2\.55/)).toBeVisible();
    const position = page.locator(".card").filter({ hasText: "Your Position" });
    await expect(position.getByText("$2.55")).toBeVisible();
    await expect(page.getByText("$12.55")).toBeVisible();
  });

  test("withdraw validates over-balance locally", async ({ page }) => {
    await page.getByRole("button", { name: "Connect Wallet" }).first().click();
    await page.getByRole("button", { name: "Withdraw" }).click();
    await page.getByPlaceholder("0.00").fill("100");
    await expect(page.getByText(/Exceeds available \$0/)).toBeVisible();
  });

  test("shows the admin indicator and renders a recorded winner in the feed", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Connect Wallet" }).first().click();
    await expect(page.getByText("You are the admin")).toBeVisible();

    await expect(page.getByText("No draws yet")).toBeVisible();
    await page.getByRole("button", { name: "Execute Prize Draw" }).click();
    await expect(page.getByText(/Draw executed!/)).toBeVisible();
    const feed = page.locator(".card").filter({ hasText: "Live Draw Feed" });
    await expect(feed.getByText(SHORT_ADDR)).toBeVisible();
  });
});

test.describe("Aqua vault UI (connected as non-admin)", () => {
  test("does not show the admin indicator", async ({ page }) => {
    await setup(page, OTHER_PUBKEY);
    await page.getByRole("button", { name: "Connect Wallet" }).first().click();
    await expect(page.getByText("You are the admin")).not.toBeVisible();
    await expect(page.getByText("Connect Wallet").first()).not.toBeVisible();
  });
});
