/**
 * NekoPak marks — original artwork (a parcel whose flaps are cat ears),
 * drawn in the current accent. Unrelated to PikPak's or Piko's logos.
 */

export function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <img class="logo-mark" src="/brand/app-icon.svg" width={size} height={size} alt="" aria-hidden="true" draggable={false} />
  );
}

/** A cat napping in an open box — empty states only. */
export function BoxCat({ size = 128, awake = false }: { size?: number; awake?: boolean }) {
  return (
    <svg class="boxcat" width={size} height={(size * 84) / 140} viewBox="0 0 140 84" aria-hidden="true" focusable="false" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M24 47h92v29a3 3 0 0 1-3 3H27a3 3 0 0 1-3-3Z" fill="var(--surface-high)" />
      <path d="M44 46c0-14 11-22 26-22s26 8 26 22" fill="var(--surface-lowest)" />
      <path d="M48 33 46 17l13 9M92 33l2-16-13 9" fill="var(--surface-lowest)" />
      {awake ? (
        <>
          <circle cx="62" cy="39" r="1.8" fill="currentColor" stroke="none" />
          <circle cx="78" cy="39" r="1.8" fill="currentColor" stroke="none" />
        </>
      ) : (
        <path d="M58 40q3.5-3 7 0M75 40q3.5-3 7 0" />
      )}
      <path d="M68 44.5h4l-2 2Z" fill="currentColor" stroke-width="1" />
      <path d="M52 47q5-6 10 0M78 47q5-6 10 0" fill="var(--surface-lowest)" />
      <path d="M24 47 12 37M116 47l12-10" />
    </svg>
  );
}
