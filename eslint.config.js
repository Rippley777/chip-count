import js from '@eslint/js';
import ts from 'typescript-eslint';
export default ts.config(js.configs.recommended, ...ts.configs.recommended, {
  files:['src/**/*.{ts,tsx}'],
  languageOptions:{globals:{window:'readonly',document:'readonly',navigator:'readonly',localStorage:'readonly',console:'readonly',setTimeout:'readonly',setInterval:'readonly',clearTimeout:'readonly',clearInterval:'readonly',fetch:'readonly',Blob:'readonly',URL:'readonly',HTMLElement:'readonly',HTMLInputElement:'readonly',HTMLDivElement:'readonly',KeyboardEvent:'readonly',MouseEvent:'readonly',ResizeObserver:'readonly',requestAnimationFrame:'readonly',cancelAnimationFrame:'readonly',Event:'readonly',HTMLSelectElement:'readonly',HTMLButtonElement:'readonly',IntersectionObserver:'readonly'}},
  rules:{'@typescript-eslint/no-unused-vars':['warn',{argsIgnorePattern:'^_',varsIgnorePattern:'^_'}],'@typescript-eslint/no-explicit-any':'warn','no-undef':'off'}
});
