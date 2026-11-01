import { getCurrentWindow } from '@tauri-apps/api/window'
import { Minus, Power, Speaker, Square, X } from 'lucide-react'
import { keyLabel } from '../lib/hotkeys'
import { currentOutput, dismiss, setState, updateConfig, listed, useStore, works } from '../lib/store'
import { useT } from '../i18n'
import { Select } from './controls'
import { Logo } from './ui'

const win = getCurrentWindow()

/** Barra de título: la salida que se edita (el micrófono tiene su selector en su página) y el bypass del EQ */
export function TitleBar(): React.JSX.Element {
  const t = useT()
  const devices = useStore((s) => s.engine?.devices)
  const device = useStore((s) => currentOutput(s))
  const off = useStore((s) => !!s.config?.bypass)
  const hotkey = useStore((s) => s.config?.hotkeys.bypass)
  const list = (devices ?? []).filter((d) => !d.input && !d.unplugged && listed(d))
  const toggle = (): void => {
    const cfg = useStore.getState().config
    if (cfg) updateConfig({ bypass: !cfg.bypass })
  }
  const tip = `${t(off ? 'titlebar.bypassOn' : 'titlebar.bypassOff')}${hotkey ? ` · ${keyLabel(hotkey)}` : ''}`

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span data-tauri-drag-region>
          Poxi<b>EQ</b>
        </span>
      </div>
      {device && (
        <div className="no-drag device-pick">
          <span className={`dot ${works(device) ? 'ok' : device.broken ? 'bad' : ''}`} />
          <span className="kind" data-tip={t('titlebar.outputTip')}>
            <Speaker size={15} />
          </span>
          <Select
            size="sm"
            value={device.id}
            label={t('titlebar.device')}
            options={list.map((d) => ({
              value: d.id,
              label: `${d.name} · ${d.device}${d.default ? ` (${t('device.default')})` : ''}${d.pair ? ` (${t('device.calls')})` : ''}`
            }))}
            onChange={(id) => {
              setState({ selected: null })
              updateConfig({ device: id })
            }}
          />
        </div>
      )}
      <div className="spacer" data-tauri-drag-region />
      <button className={`power no-drag ${off ? 'bypassed' : ''}`} onClick={toggle} data-tip={tip} aria-label={tip} aria-pressed={!off}>
        <Power size={15} />
        <span>EQ</span>
      </button>
      <div className="win-controls">
        <button onClick={() => void win.minimize()} aria-label={t('titlebar.minimize')}>
          <Minus size={16} />
        </button>
        <button onClick={() => void win.toggleMaximize()} aria-label={t('titlebar.maximize')}>
          <Square size={12} />
        </button>
        <button className="close" onClick={() => void win.close()} aria-label={t('titlebar.close')}>
          <X size={17} />
        </button>
      </div>
    </header>
  )
}

export function Toasts(): React.JSX.Element {
  const toasts = useStore((s) => s.toasts)
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((x) => (
        <div key={x.id} className={`toast ${x.kind}`}>
          <ToastIcon kind={x.kind} />
          {x.text}
          {x.action && (
            <button
              className="btn btn-sm toast-action"
              onClick={() => {
                dismiss(x.id)
                x.action?.run()
              }}
            >
              {x.action.label}
            </button>
          )}
          {x.secs ? <span className="bar" style={{ animationDuration: `${x.secs}s` }} /> : null}
        </div>
      ))}
    </div>
  )
}

function ToastIcon({ kind }: { kind: 'ok' | 'err' | 'info' }): React.JSX.Element {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
      {kind === 'ok' && <path d="M20 6 9 17l-5-5" />}
      {kind === 'err' && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 8v4M12 16h.01" />
        </>
      )}
      {kind === 'info' && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 16v-4M12 8h.01" />
        </>
      )}
    </svg>
  )
}
