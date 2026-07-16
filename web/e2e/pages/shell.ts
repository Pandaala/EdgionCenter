import type { Page } from '@playwright/test'
export class ShellPage {
  constructor(private readonly page: Page) {}
  async selectController(slot: 'A' | 'B'): Promise<void> { await this.page.getByTestId('controller-selector').click(); await this.page.getByTestId(`controller-${slot}`).click() }
}
