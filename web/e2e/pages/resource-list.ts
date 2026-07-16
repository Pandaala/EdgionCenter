import type { Page } from '@playwright/test'

export class ResourceListPage {
  constructor(private readonly page: Page, private readonly kind: string) {}
  async open(controller: string, route: string): Promise<void> { await this.page.goto(`/controller/${encodeURIComponent(controller)}/${route}`) }
  refresh() { return this.page.getByTestId(`${this.kind}-refresh`).click() }
  create() { return this.page.getByTestId(`${this.kind}-create`).click() }
  rowView() { return this.page.getByTestId(`${this.kind}-row-view`).first().click() }
}
