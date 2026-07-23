import { useState } from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import LinkSysForm from './LinkSysForm'
import { createEmpty } from '@/utils/linksys'
import type { LinkSys } from '@/types/link-sys'

function Harness(){const [value,setValue]=useState<LinkSys>(createEmpty());return <LinkSysForm data={value} onChange={setValue}/>}
describe('LinkSysForm variant drafts',()=>{
  it('restores an edited variant after switching away and back',async()=>{
    render(<Harness/>)
    const dbItem=screen.getByText('Database Number').closest('.ant-form-item')!
    fireEvent.change(dbItem.querySelector('input')!,{target:{value:'3'}})
    const typeInput=screen.getAllByRole('combobox')[0]
    fireEvent.mouseDown(typeInput);fireEvent.click((await screen.findAllByText('Kafka')).at(-1)!)
    fireEvent.mouseDown(screen.getAllByRole('combobox')[0]);fireEvent.click((await screen.findAllByText('Redis')).at(-1)!)
    const restored=screen.getByText('Database Number').closest('.ant-form-item')!.querySelector('input')!
    expect(restored).toHaveValue('3')
  })

  it('does not expose removed webhook degradation controls', async () => {
    render(<Harness/>)
    fireEvent.mouseDown(screen.getAllByRole('combobox')[0])
    fireEvent.click((await screen.findAllByText('Webhook')).at(-1)!)
    expect(screen.queryByText('Allow degradation')).not.toBeInTheDocument()
    expect(screen.queryByText('Degradation template')).not.toBeInTheDocument()
  })
})
